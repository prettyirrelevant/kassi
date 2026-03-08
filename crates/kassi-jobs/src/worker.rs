use std::future::Future;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

use crate::error::JobError;
use crate::job::Job;

pub trait JobHandler: Send + Sync + 'static {
    fn handle(&self, job: &Job) -> impl Future<Output = Result<(), JobError>> + Send;
}

pub struct WorkerConfig {
    pub queue: String,
    pub poll_interval: Duration,
    pub base_backoff: Duration,
}

impl WorkerConfig {
    pub fn new(queue: impl Into<String>) -> Self {
        Self {
            queue: queue.into(),
            poll_interval: Duration::from_secs(5),
            base_backoff: Duration::from_secs(5),
        }
    }

    #[must_use]
    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    #[must_use]
    pub fn base_backoff(mut self, backoff: Duration) -> Self {
        self.base_backoff = backoff;
        self
    }
}

pub struct Worker<H> {
    pool: PgPool,
    handler: H,
    config: WorkerConfig,
    cancel: CancellationToken,
}

impl<H: JobHandler> Worker<H> {
    pub fn new(pool: PgPool, handler: H, config: WorkerConfig) -> Self {
        Self {
            pool,
            handler,
            config,
            cancel: CancellationToken::new(),
        }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Starts the poll loop: recovers stale jobs, then continuously
    /// polls for new work until the cancellation token is triggered.
    ///
    /// # Errors
    /// Returns `JobError::Sqlx` on database failure.
    pub async fn run(self) -> Result<(), JobError> {
        let recovered = Job::recover_stale(&self.pool, &self.config.queue).await?;
        if recovered > 0 {
            tracing::info!(
                queue = %self.config.queue,
                count = recovered,
                "recovered stale running jobs"
            );
        }

        tracing::info!(queue = %self.config.queue, "worker started");

        loop {
            if self.cancel.is_cancelled() {
                tracing::info!(queue = %self.config.queue, "worker shutting down");
                return Ok(());
            }

            match Job::poll(&self.pool, &self.config.queue).await? {
                Some(job) => self.process(job).await?,
                None => {
                    tokio::select! {
                        () = self.cancel.cancelled() => {},
                        () = tokio::time::sleep(self.config.poll_interval) => {}
                    }
                }
            }
        }
    }

    async fn process(&self, job: Job) -> Result<(), JobError> {
        let job_id = job.id;
        tracing::info!(
            job_id,
            queue = %self.config.queue,
            attempt = job.attempts,
            "processing job"
        );

        match self.handler.handle(&job).await {
            Ok(()) => {
                Job::complete(&self.pool, job_id).await?;
                tracing::info!(job_id, "job completed");
            }
            Err(e) => {
                let error_msg = e.to_string();
                if job.attempts >= job.max_attempts {
                    Job::dead_letter(&self.pool, job_id, &error_msg).await?;
                    tracing::warn!(job_id, error = %error_msg, "job moved to dead letter");
                } else {
                    let backoff_secs = self.config.base_backoff.as_secs()
                        * 2u64.saturating_pow(job.attempts.saturating_sub(1).cast_unsigned());
                    let retry_at =
                        Utc::now() + chrono::Duration::seconds(backoff_secs.cast_signed());

                    Job::fail(&self.pool, job_id, &error_msg, retry_at).await?;
                    tracing::warn!(
                        job_id,
                        error = %error_msg,
                        attempt = job.attempts,
                        retry_at = %retry_at,
                        "job failed, scheduled for retry"
                    );
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    struct CountingHandler {
        count: Arc<AtomicU32>,
    }

    impl JobHandler for CountingHandler {
        async fn handle(&self, _job: &Job) -> Result<(), JobError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct FailingHandler;

    impl JobHandler for FailingHandler {
        async fn handle(&self, _job: &Job) -> Result<(), JobError> {
            Err(JobError::Handler("always fails".into()))
        }
    }

    #[sqlx::test(migrations = "../kassi-db/migrations")]
    async fn worker_processes_enqueued_job(pool: PgPool) {
        let count = Arc::new(AtomicU32::new(0));

        Job::enqueue(&pool, "test", &serde_json::json!("data"), 3)
            .await
            .unwrap();

        let config = WorkerConfig::new("test").poll_interval(Duration::from_millis(50));
        let worker = Worker::new(
            pool,
            CountingHandler {
                count: Arc::clone(&count),
            },
            config,
        );
        let token = worker.cancellation_token();

        let handle = tokio::spawn(async move { worker.run().await });

        tokio::time::sleep(Duration::from_millis(200)).await;
        token.cancel();
        handle.await.unwrap().unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[sqlx::test(migrations = "../kassi-db/migrations")]
    async fn worker_dead_letters_after_max_attempts(pool: PgPool) {
        Job::enqueue(&pool, "fail_q", &serde_json::json!("data"), 1)
            .await
            .unwrap();

        let config = WorkerConfig::new("fail_q").poll_interval(Duration::from_millis(50));
        let worker = Worker::new(pool.clone(), FailingHandler, config);
        let token = worker.cancellation_token();

        let handle = tokio::spawn(async move { worker.run().await });

        tokio::time::sleep(Duration::from_millis(200)).await;
        token.cancel();
        handle.await.unwrap().unwrap();

        let row = sqlx::query_as::<_, Job>("SELECT * FROM jobs WHERE queue = 'fail_q'")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(row.status, "dead");
        assert_eq!(
            row.last_error.as_deref(),
            Some("handler error: always fails")
        );
    }

    #[sqlx::test(migrations = "../kassi-db/migrations")]
    async fn worker_retries_failed_job_with_backoff(pool: PgPool) {
        Job::enqueue(&pool, "retry_q", &serde_json::json!("data"), 3)
            .await
            .unwrap();

        let config = WorkerConfig::new("retry_q")
            .poll_interval(Duration::from_millis(50))
            .base_backoff(Duration::from_secs(60));
        let worker = Worker::new(pool.clone(), FailingHandler, config);
        let token = worker.cancellation_token();

        let handle = tokio::spawn(async move { worker.run().await });

        tokio::time::sleep(Duration::from_millis(200)).await;
        token.cancel();
        handle.await.unwrap().unwrap();

        let row = sqlx::query_as::<_, Job>("SELECT * FROM jobs WHERE queue = 'retry_q'")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(row.status, "pending");
        assert_eq!(row.attempts, 1);
        assert!(row.scheduled_at > Utc::now());
    }

    #[sqlx::test(migrations = "../kassi-db/migrations")]
    async fn worker_recovers_stale_jobs_on_startup(pool: PgPool) {
        Job::enqueue(&pool, "recover_q", &serde_json::json!(null), 3)
            .await
            .unwrap();
        Job::poll(&pool, "recover_q").await.unwrap().unwrap();

        let count = Arc::new(AtomicU32::new(0));
        let config = WorkerConfig::new("recover_q").poll_interval(Duration::from_millis(50));
        let worker = Worker::new(
            pool,
            CountingHandler {
                count: Arc::clone(&count),
            },
            config,
        );
        let token = worker.cancellation_token();

        let handle = tokio::spawn(async move { worker.run().await });

        tokio::time::sleep(Duration::from_millis(300)).await;
        token.cancel();
        handle.await.unwrap().unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[sqlx::test(migrations = "../kassi-db/migrations")]
    async fn worker_shuts_down_gracefully(pool: PgPool) {
        let config = WorkerConfig::new("shutdown_q").poll_interval(Duration::from_millis(50));
        let worker = Worker::new(
            pool,
            CountingHandler {
                count: Arc::new(AtomicU32::new(0)),
            },
            config,
        );
        let token = worker.cancellation_token();

        let handle = tokio::spawn(async move { worker.run().await });

        token.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), handle).await;

        assert!(result.is_ok(), "worker should shut down within timeout");
        result.unwrap().unwrap().unwrap();
    }
}
