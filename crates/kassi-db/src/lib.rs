pub mod models;
pub mod schema;

use diesel_async::pooled_connection::bb8::Pool;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::AsyncPgConnection;

pub type DbPool = Pool<AsyncPgConnection>;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("pool error: {0}")]
    Pool(String),
}

/// Creates a bb8 connection pool for the given Postgres database.
///
/// # Errors
/// Returns `DbError::Pool` if the pool cannot be built.
pub async fn create_pool(database_url: &str) -> Result<DbPool, DbError> {
    let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(database_url);
    Pool::builder()
        .max_size(10)
        .build(config)
        .await
        .map_err(|e| DbError::Pool(e.to_string()))
}
