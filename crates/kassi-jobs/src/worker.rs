use std::future::Future;
use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use kassi_db::models::Job;
use kassi_db::DbPool;

use crate::error::JobError;
use crate::job;

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
    pool: DbPool,
    handler: H,
    config: WorkerConfig,
    cancel: CancellationToken,
}

impl<H: JobHandler> Worker<H> {
    pub fn new(pool: DbPool, handler: H, config: WorkerConfig) -> Self {
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
    /// Returns `JobError` on database failure.
    pub async fn run(self) -> Result<(), JobError> {
        let recovered = job::recover_stale(&self.pool, &self.config.queue).await?;
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

            match job::poll(&self.pool, &self.config.queue).await? {
                Some(j) => self.process(j).await?,
                None => {
                    tokio::select! {
                        () = self.cancel.cancelled() => {},
                        () = tokio::time::sleep(self.config.poll_interval) => {}
                    }
                }
            }
        }
    }

    async fn process(&self, j: Job) -> Result<(), JobError> {
        let job_id = j.id;
        tracing::info!(
            job_id,
            queue = %self.config.queue,
            attempt = j.attempts,
            "processing job"
        );

        match self.handler.handle(&j).await {
            Ok(()) => {
                job::complete(&self.pool, job_id).await?;
                tracing::info!(job_id, "job completed");
            }
            Err(e) => {
                let error_msg = e.to_string();
                if j.attempts >= j.max_attempts {
                    job::dead_letter(&self.pool, job_id, &error_msg).await?;
                    tracing::warn!(job_id, error = %error_msg, "job moved to dead letter");
                } else {
                    let backoff_secs = self.config.base_backoff.as_secs()
                        * 2u64.saturating_pow(j.attempts.saturating_sub(1).cast_unsigned());
                    let retry_at =
                        Utc::now() + chrono::Duration::seconds(backoff_secs.cast_signed());

                    job::fail(&self.pool, job_id, &error_msg, retry_at).await?;
                    tracing::warn!(
                        job_id,
                        error = %error_msg,
                        attempt = j.attempts,
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
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    pub(super) async fn test_pool() -> DbPool {
        kassi_db::create_pool(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
            .await
            .expect("failed to create test pool")
    }

    struct CountingHandler {
        count: Arc<AtomicU32>,
    }

    impl JobHandler for CountingHandler {
        async fn handle(&self, _job: &Job) -> Result<(), JobError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    pub(super) struct FailingHandler;

    impl JobHandler for FailingHandler {
        async fn handle(&self, _job: &Job) -> Result<(), JobError> {
            Err(JobError::Handler("always fails".into()))
        }
    }

    #[tokio::test]
    async fn worker_processes_enqueued_job() {
        let pool = test_pool().await;
        let count = Arc::new(AtomicU32::new(0));

        job::enqueue(&pool, "w_test", &serde_json::json!("data"), 3)
            .await
            .unwrap();

        let config = WorkerConfig::new("w_test").poll_interval(Duration::from_millis(50));
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

    #[tokio::test]
    async fn worker_recovers_stale_jobs_on_startup() {
        let pool = test_pool().await;

        job::enqueue(&pool, "w_recover_q", &serde_json::json!(null), 3)
            .await
            .unwrap();
        job::poll(&pool, "w_recover_q").await.unwrap().unwrap();

        let count = Arc::new(AtomicU32::new(0));
        let config = WorkerConfig::new("w_recover_q").poll_interval(Duration::from_millis(50));
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

    #[tokio::test]
    async fn worker_shuts_down_gracefully() {
        let pool = test_pool().await;

        let config = WorkerConfig::new("w_shutdown_q").poll_interval(Duration::from_millis(50));
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

    // Tests that assert directly against the DB need diesel imports,
    // which are in a submodule to avoid `RunQueryDsl::load` shadowing
    // `AtomicU32::load` in the parent module.
    mod db_assertions {
        use std::time::Duration;

        use chrono::Utc;
        use diesel::prelude::*;
        use diesel_async::RunQueryDsl;
        use kassi_db::models::Job;
        use kassi_db::schema::jobs;

        use crate::job;
        use crate::worker::tests::{test_pool, FailingHandler};
        use crate::worker::{Worker, WorkerConfig};

        #[tokio::test]
        async fn worker_dead_letters_after_max_attempts() {
            let pool = test_pool().await;

            job::enqueue(&pool, "w_fail_q", &serde_json::json!("data"), 1)
                .await
                .unwrap();

            let config = WorkerConfig::new("w_fail_q").poll_interval(Duration::from_millis(50));
            let worker = Worker::new(pool.clone(), FailingHandler, config);
            let token = worker.cancellation_token();

            let handle = tokio::spawn(async move { worker.run().await });

            tokio::time::sleep(Duration::from_millis(200)).await;
            token.cancel();
            handle.await.unwrap().unwrap();

            let mut conn = pool.get().await.unwrap();
            let row: Job = jobs::table
                .filter(jobs::queue.eq("w_fail_q"))
                .select(Job::as_select())
                .first(&mut conn)
                .await
                .unwrap();

            assert_eq!(row.status, "dead");
            assert_eq!(
                row.last_error.as_deref(),
                Some("handler error: always fails")
            );
        }

        #[tokio::test]
        async fn worker_retries_failed_job_with_backoff() {
            let pool = test_pool().await;

            job::enqueue(&pool, "w_retry_q", &serde_json::json!("data"), 3)
                .await
                .unwrap();

            let config = WorkerConfig::new("w_retry_q")
                .poll_interval(Duration::from_millis(50))
                .base_backoff(Duration::from_secs(60));
            let worker = Worker::new(pool.clone(), FailingHandler, config);
            let token = worker.cancellation_token();

            let handle = tokio::spawn(async move { worker.run().await });

            tokio::time::sleep(Duration::from_millis(200)).await;
            token.cancel();
            handle.await.unwrap().unwrap();

            let mut conn = pool.get().await.unwrap();
            let row: Job = jobs::table
                .filter(jobs::queue.eq("w_retry_q"))
                .select(Job::as_select())
                .first(&mut conn)
                .await
                .unwrap();

            assert_eq!(row.status, "pending");
            assert_eq!(row.attempts, 1);
            assert!(row.scheduled_at > Utc::now());
        }
    }
}
