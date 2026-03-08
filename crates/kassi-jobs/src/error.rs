#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("database error: {0}")]
    Diesel(#[from] diesel::result::Error),

    #[error("pool error: {0}")]
    Pool(String),

    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("handler error: {0}")]
    Handler(String),
}

impl From<bb8::RunError<diesel_async::pooled_connection::PoolError>> for JobError {
    fn from(e: bb8::RunError<diesel_async::pooled_connection::PoolError>) -> Self {
        Self::Pool(e.to_string())
    }
}
