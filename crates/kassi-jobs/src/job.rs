use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::Text;
use diesel_async::RunQueryDsl;

use kassi_db::models::{Job, NewJob};
use kassi_db::schema::jobs;
use kassi_db::DbPool;

use crate::error::JobError;

/// Inserts a new job into the given queue.
///
/// # Errors
/// Returns `JobError::Json` on serialization failure,
/// or `JobError::Diesel` on database failure.
pub async fn enqueue(
    pool: &DbPool,
    queue: &str,
    payload: &impl serde::Serialize,
    max_attempts: i32,
) -> Result<Job, JobError> {
    let new_job = NewJob {
        queue,
        payload: serde_json::to_value(payload)?,
        max_attempts,
        scheduled_at: None,
    };

    let mut conn = pool.get().await?;
    Ok(diesel::insert_into(jobs::table)
        .values(&new_job)
        .returning(Job::as_returning())
        .get_result(&mut conn)
        .await?)
}

/// Inserts a new job scheduled to run at a specific time.
///
/// # Errors
/// Returns `JobError::Json` on serialization failure,
/// or `JobError::Diesel` on database failure.
pub async fn enqueue_scheduled(
    pool: &DbPool,
    queue: &str,
    payload: &impl serde::Serialize,
    max_attempts: i32,
    scheduled_at: DateTime<Utc>,
) -> Result<Job, JobError> {
    let new_job = NewJob {
        queue,
        payload: serde_json::to_value(payload)?,
        max_attempts,
        scheduled_at: Some(scheduled_at),
    };

    let mut conn = pool.get().await?;
    Ok(diesel::insert_into(jobs::table)
        .values(&new_job)
        .returning(Job::as_returning())
        .get_result(&mut conn)
        .await?)
}

/// Atomically claims the next pending job from the given queue.
/// Uses `FOR UPDATE SKIP LOCKED` to allow concurrent workers
/// without blocking each other.
///
/// # Errors
/// Returns `JobError::Diesel` on database failure.
pub async fn poll(pool: &DbPool, queue: &str) -> Result<Option<Job>, JobError> {
    let mut conn = pool.get().await?;
    Ok(diesel::sql_query(
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
    .bind::<Text, _>(queue)
    .get_result(&mut conn)
    .await
    .optional()?)
}

/// Marks a job as completed.
///
/// # Errors
/// Returns `JobError::Diesel` on database failure.
pub async fn complete(pool: &DbPool, id: i64) -> Result<Job, JobError> {
    let mut conn = pool.get().await?;
    Ok(diesel::update(jobs::table.find(id))
        .set((
            jobs::status.eq("completed"),
            jobs::completed_at.eq(diesel::dsl::now),
        ))
        .returning(Job::as_returning())
        .get_result(&mut conn)
        .await?)
}

/// Records a failure and reschedules the job for retry at `retry_at`.
///
/// # Errors
/// Returns `JobError::Diesel` on database failure.
pub async fn fail(
    pool: &DbPool,
    id: i64,
    error: &str,
    retry_at: DateTime<Utc>,
) -> Result<Job, JobError> {
    let mut conn = pool.get().await?;
    Ok(diesel::update(jobs::table.find(id))
        .set((
            jobs::status.eq("pending"),
            jobs::failed_at.eq(diesel::dsl::now),
            jobs::last_error.eq(error),
            jobs::scheduled_at.eq(retry_at),
        ))
        .returning(Job::as_returning())
        .get_result(&mut conn)
        .await?)
}

/// Permanently marks a job as dead after exhausting all retries.
///
/// # Errors
/// Returns `JobError::Diesel` on database failure.
pub async fn dead_letter(pool: &DbPool, id: i64, error: &str) -> Result<Job, JobError> {
    let mut conn = pool.get().await?;
    Ok(diesel::update(jobs::table.find(id))
        .set((
            jobs::status.eq("dead"),
            jobs::failed_at.eq(diesel::dsl::now),
            jobs::last_error.eq(error),
        ))
        .returning(Job::as_returning())
        .get_result(&mut conn)
        .await?)
}

/// Resets stale running jobs back to pending on startup.
///
/// # Errors
/// Returns `JobError::Diesel` on database failure.
pub async fn recover_stale(pool: &DbPool, queue: &str) -> Result<u64, JobError> {
    let mut conn = pool.get().await?;
    let count = diesel::update(
        jobs::table
            .filter(jobs::queue.eq(queue))
            .filter(jobs::status.eq("running")),
    )
    .set((
        jobs::status.eq("pending"),
        jobs::started_at.eq(None::<DateTime<Utc>>),
    ))
    .execute(&mut conn)
    .await?;

    Ok(count as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> DbPool {
        kassi_db::create_pool(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
            .await
            .expect("failed to create test pool")
    }

    mod enqueue_tests {
        use super::*;

        #[tokio::test]
        async fn inserts_pending_job() {
            let pool = test_pool().await;
            let job = enqueue(&pool, "test_queue", &serde_json::json!({"key": "value"}), 3)
                .await
                .unwrap();

            assert_eq!(job.queue, "test_queue");
            assert_eq!(job.status, "pending");
            assert_eq!(job.attempts, 0);
            assert_eq!(job.max_attempts, 3);
            assert_eq!(job.payload, serde_json::json!({"key": "value"}));
        }

        #[tokio::test]
        async fn uses_default_max_attempts_from_caller() {
            let pool = test_pool().await;
            let job = enqueue(&pool, "q", &serde_json::json!(null), 10)
                .await
                .unwrap();
            assert_eq!(job.max_attempts, 10);
        }
    }

    mod enqueue_scheduled_tests {
        use super::*;

        #[tokio::test]
        async fn schedules_job_in_the_future() {
            let pool = test_pool().await;
            let future = Utc::now() + chrono::Duration::hours(1);
            let job = enqueue_scheduled(&pool, "q", &serde_json::json!(null), 3, future)
                .await
                .unwrap();

            assert!(job.scheduled_at > Utc::now());
        }
    }

    mod poll_tests {
        use super::*;

        #[tokio::test]
        async fn returns_none_when_queue_empty() {
            let pool = test_pool().await;
            let job = poll(&pool, "empty_queue").await.unwrap();
            assert!(job.is_none());
        }

        #[tokio::test]
        async fn claims_pending_job() {
            let pool = test_pool().await;
            let enqueued = enqueue(&pool, "poll_claim", &serde_json::json!("hello"), 3)
                .await
                .unwrap();

            let polled = poll(&pool, "poll_claim").await.unwrap().unwrap();

            assert_eq!(polled.id, enqueued.id);
            assert_eq!(polled.status, "running");
            assert_eq!(polled.attempts, 1);
            assert!(polled.started_at.is_some());
        }

        #[tokio::test]
        async fn skips_future_scheduled_jobs() {
            let pool = test_pool().await;
            let future = Utc::now() + chrono::Duration::hours(1);
            enqueue_scheduled(&pool, "future_q", &serde_json::json!(null), 3, future)
                .await
                .unwrap();

            let polled = poll(&pool, "future_q").await.unwrap();
            assert!(polled.is_none());
        }

        #[tokio::test]
        async fn does_not_return_already_running_job() {
            let pool = test_pool().await;
            enqueue(&pool, "running_q", &serde_json::json!(null), 3)
                .await
                .unwrap();

            let first = poll(&pool, "running_q").await.unwrap();
            assert!(first.is_some());

            let second = poll(&pool, "running_q").await.unwrap();
            assert!(second.is_none());
        }
    }

    mod complete_tests {
        use super::*;

        #[tokio::test]
        async fn marks_job_completed() {
            let pool = test_pool().await;
            let job = enqueue(&pool, "complete_q", &serde_json::json!(null), 3)
                .await
                .unwrap();
            let polled = poll(&pool, "complete_q").await.unwrap().unwrap();
            let completed = complete(&pool, polled.id).await.unwrap();

            assert_eq!(completed.id, job.id);
            assert_eq!(completed.status, "completed");
            assert!(completed.completed_at.is_some());
        }
    }

    mod fail_tests {
        use super::*;

        #[tokio::test]
        async fn reschedules_job_as_pending() {
            let pool = test_pool().await;
            let job = enqueue(&pool, "fail_q", &serde_json::json!(null), 3)
                .await
                .unwrap();
            poll(&pool, "fail_q").await.unwrap().unwrap();

            let retry_at = Utc::now() + chrono::Duration::seconds(30);
            let failed = fail(&pool, job.id, "boom", retry_at).await.unwrap();

            assert_eq!(failed.status, "pending");
            assert_eq!(failed.last_error.as_deref(), Some("boom"));
            assert!(failed.failed_at.is_some());
            assert!(failed.scheduled_at >= retry_at - chrono::Duration::seconds(1));
        }
    }

    mod dead_letter_tests {
        use super::*;

        #[tokio::test]
        async fn marks_job_dead() {
            let pool = test_pool().await;
            let job = enqueue(&pool, "dead_q", &serde_json::json!(null), 1)
                .await
                .unwrap();
            poll(&pool, "dead_q").await.unwrap().unwrap();

            let dead = dead_letter(&pool, job.id, "gave up").await.unwrap();

            assert_eq!(dead.status, "dead");
            assert_eq!(dead.last_error.as_deref(), Some("gave up"));
        }
    }

    mod recover_stale_tests {
        use super::*;

        #[tokio::test]
        async fn resets_running_jobs_to_pending() {
            let pool = test_pool().await;
            enqueue(&pool, "recover_q", &serde_json::json!(null), 3)
                .await
                .unwrap();
            poll(&pool, "recover_q").await.unwrap().unwrap();

            let recovered = recover_stale(&pool, "recover_q").await.unwrap();
            assert_eq!(recovered, 1);

            let repoll = poll(&pool, "recover_q").await.unwrap();
            assert!(repoll.is_some());
        }

        #[tokio::test]
        async fn ignores_other_queues() {
            let pool = test_pool().await;
            enqueue(&pool, "recover_q1", &serde_json::json!(null), 3)
                .await
                .unwrap();
            poll(&pool, "recover_q1").await.unwrap().unwrap();

            let recovered = recover_stale(&pool, "recover_q2").await.unwrap();
            assert_eq!(recovered, 0);
        }
    }
}
