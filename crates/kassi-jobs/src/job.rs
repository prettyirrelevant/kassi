use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::error::JobError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Job {
    pub id: i64,
    pub queue: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub scheduled_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub failed_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Job {
    /// Inserts a new job into the given queue.
    ///
    /// # Errors
    /// Returns `JobError::Json` if the payload can't be serialized,
    /// or `JobError::Sqlx` on database failure.
    pub async fn enqueue(
        pool: &PgPool,
        queue: &str,
        payload: &impl Serialize,
        max_attempts: i32,
    ) -> Result<Self, JobError> {
        Ok(sqlx::query_as::<_, Self>(
            "INSERT INTO jobs (queue, payload, max_attempts)
             VALUES ($1, $2, $3)
             RETURNING *",
        )
        .bind(queue)
        .bind(serde_json::to_value(payload)?)
        .bind(max_attempts)
        .fetch_one(pool)
        .await?)
    }

    /// Inserts a new job scheduled to run at a specific time.
    ///
    /// # Errors
    /// Returns `JobError::Json` if the payload can't be serialized,
    /// or `JobError::Sqlx` on database failure.
    pub async fn enqueue_scheduled(
        pool: &PgPool,
        queue: &str,
        payload: &impl Serialize,
        max_attempts: i32,
        scheduled_at: DateTime<Utc>,
    ) -> Result<Self, JobError> {
        Ok(sqlx::query_as::<_, Self>(
            "INSERT INTO jobs (queue, payload, max_attempts, scheduled_at)
             VALUES ($1, $2, $3, $4)
             RETURNING *",
        )
        .bind(queue)
        .bind(serde_json::to_value(payload)?)
        .bind(max_attempts)
        .bind(scheduled_at)
        .fetch_one(pool)
        .await?)
    }

    /// Atomically claims the next pending job from the given queue.
    /// Uses `FOR UPDATE SKIP LOCKED` to allow concurrent workers
    /// without blocking each other.
    ///
    /// # Errors
    /// Returns `JobError::Sqlx` on database failure.
    pub async fn poll(pool: &PgPool, queue: &str) -> Result<Option<Self>, JobError> {
        Ok(sqlx::query_as::<_, Self>(
            "UPDATE jobs SET status = 'running', started_at = now(), attempts = attempts + 1
             WHERE id = (
                 SELECT id FROM jobs
                 WHERE queue = $1 AND status = 'pending' AND scheduled_at <= now()
                 ORDER BY scheduled_at ASC
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED
             )
             RETURNING *",
        )
        .bind(queue)
        .fetch_optional(pool)
        .await?)
    }

    /// Marks a job as completed.
    ///
    /// # Errors
    /// Returns `JobError::Sqlx` on database failure.
    pub async fn complete(pool: &PgPool, id: i64) -> Result<Self, JobError> {
        Ok(sqlx::query_as::<_, Self>(
            "UPDATE jobs SET status = 'completed', completed_at = now()
             WHERE id = $1
             RETURNING *",
        )
        .bind(id)
        .fetch_one(pool)
        .await?)
    }

    /// Records a failure and reschedules the job for retry at `retry_at`.
    ///
    /// # Errors
    /// Returns `JobError::Sqlx` on database failure.
    pub async fn fail(
        pool: &PgPool,
        id: i64,
        error: &str,
        retry_at: DateTime<Utc>,
    ) -> Result<Self, JobError> {
        Ok(sqlx::query_as::<_, Self>(
            "UPDATE jobs
             SET status = 'pending',
                 failed_at = now(),
                 last_error = $2,
                 scheduled_at = $3
             WHERE id = $1
             RETURNING *",
        )
        .bind(id)
        .bind(error)
        .bind(retry_at)
        .fetch_one(pool)
        .await?)
    }

    /// Permanently marks a job as dead after exhausting all retries.
    ///
    /// # Errors
    /// Returns `JobError::Sqlx` on database failure.
    pub async fn dead_letter(pool: &PgPool, id: i64, error: &str) -> Result<Self, JobError> {
        Ok(sqlx::query_as::<_, Self>(
            "UPDATE jobs
             SET status = 'dead',
                 failed_at = now(),
                 last_error = $2
             WHERE id = $1
             RETURNING *",
        )
        .bind(id)
        .bind(error)
        .fetch_one(pool)
        .await?)
    }

    /// Resets stale running jobs back to pending on startup.
    /// Jobs that were mid-flight when a worker crashed get retried.
    ///
    /// # Errors
    /// Returns `JobError::Sqlx` on database failure.
    pub async fn recover_stale(pool: &PgPool, queue: &str) -> Result<u64, JobError> {
        let result = sqlx::query(
            "UPDATE jobs
             SET status = 'pending', started_at = NULL
             WHERE queue = $1 AND status = 'running'",
        )
        .bind(queue)
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    mod enqueue {
        use super::*;

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn inserts_pending_job(pool: PgPool) {
            let job = Job::enqueue(&pool, "test_queue", &serde_json::json!({"key": "value"}), 3)
                .await
                .unwrap();

            assert_eq!(job.queue, "test_queue");
            assert_eq!(job.status, "pending");
            assert_eq!(job.attempts, 0);
            assert_eq!(job.max_attempts, 3);
            assert_eq!(job.payload, serde_json::json!({"key": "value"}));
        }

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn uses_default_max_attempts_from_caller(pool: PgPool) {
            let job = Job::enqueue(&pool, "q", &serde_json::json!(null), 10)
                .await
                .unwrap();
            assert_eq!(job.max_attempts, 10);
        }
    }

    mod enqueue_scheduled {
        use super::*;

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn schedules_job_in_the_future(pool: PgPool) {
            let future = Utc::now() + chrono::Duration::hours(1);
            let job = Job::enqueue_scheduled(&pool, "q", &serde_json::json!(null), 3, future)
                .await
                .unwrap();

            assert!(job.scheduled_at > Utc::now());
        }
    }

    mod poll {
        use super::*;

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn returns_none_when_queue_empty(pool: PgPool) {
            let job = Job::poll(&pool, "empty_queue").await.unwrap();
            assert!(job.is_none());
        }

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn claims_pending_job(pool: PgPool) {
            let enqueued = Job::enqueue(&pool, "q", &serde_json::json!("hello"), 3)
                .await
                .unwrap();

            let polled = Job::poll(&pool, "q").await.unwrap().unwrap();

            assert_eq!(polled.id, enqueued.id);
            assert_eq!(polled.status, "running");
            assert_eq!(polled.attempts, 1);
            assert!(polled.started_at.is_some());
        }

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn skips_future_scheduled_jobs(pool: PgPool) {
            let future = Utc::now() + chrono::Duration::hours(1);
            Job::enqueue_scheduled(&pool, "q", &serde_json::json!(null), 3, future)
                .await
                .unwrap();

            let polled = Job::poll(&pool, "q").await.unwrap();
            assert!(polled.is_none());
        }

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn does_not_return_already_running_job(pool: PgPool) {
            Job::enqueue(&pool, "q", &serde_json::json!(null), 3)
                .await
                .unwrap();

            let first = Job::poll(&pool, "q").await.unwrap();
            assert!(first.is_some());

            let second = Job::poll(&pool, "q").await.unwrap();
            assert!(second.is_none());
        }
    }

    mod complete {
        use super::*;

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn marks_job_completed(pool: PgPool) {
            let job = Job::enqueue(&pool, "q", &serde_json::json!(null), 3)
                .await
                .unwrap();
            let polled = Job::poll(&pool, "q").await.unwrap().unwrap();
            let completed = Job::complete(&pool, polled.id).await.unwrap();

            assert_eq!(completed.id, job.id);
            assert_eq!(completed.status, "completed");
            assert!(completed.completed_at.is_some());
        }
    }

    mod fail {
        use super::*;

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn reschedules_job_as_pending(pool: PgPool) {
            let job = Job::enqueue(&pool, "q", &serde_json::json!(null), 3)
                .await
                .unwrap();
            Job::poll(&pool, "q").await.unwrap().unwrap();

            let retry_at = Utc::now() + chrono::Duration::seconds(30);
            let failed = Job::fail(&pool, job.id, "boom", retry_at).await.unwrap();

            assert_eq!(failed.status, "pending");
            assert_eq!(failed.last_error.as_deref(), Some("boom"));
            assert!(failed.failed_at.is_some());
            assert!(failed.scheduled_at >= retry_at - chrono::Duration::seconds(1));
        }
    }

    mod dead_letter {
        use super::*;

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn marks_job_dead(pool: PgPool) {
            let job = Job::enqueue(&pool, "q", &serde_json::json!(null), 1)
                .await
                .unwrap();
            Job::poll(&pool, "q").await.unwrap().unwrap();

            let dead = Job::dead_letter(&pool, job.id, "gave up").await.unwrap();

            assert_eq!(dead.status, "dead");
            assert_eq!(dead.last_error.as_deref(), Some("gave up"));
        }
    }

    mod recover_stale {
        use super::*;

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn resets_running_jobs_to_pending(pool: PgPool) {
            Job::enqueue(&pool, "q", &serde_json::json!(null), 3)
                .await
                .unwrap();
            Job::poll(&pool, "q").await.unwrap().unwrap();

            let recovered = Job::recover_stale(&pool, "q").await.unwrap();
            assert_eq!(recovered, 1);

            let repoll = Job::poll(&pool, "q").await.unwrap();
            assert!(repoll.is_some());
        }

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn ignores_other_queues(pool: PgPool) {
            Job::enqueue(&pool, "q1", &serde_json::json!(null), 3)
                .await
                .unwrap();
            Job::poll(&pool, "q1").await.unwrap().unwrap();

            let recovered = Job::recover_stale(&pool, "q2").await.unwrap();
            assert_eq!(recovered, 0);
        }
    }
}
