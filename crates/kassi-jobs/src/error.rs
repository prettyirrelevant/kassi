#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("handler error: {0}")]
    Handler(String),
}
