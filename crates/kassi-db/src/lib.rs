use sqlx::postgres::{PgPool, PgPoolOptions};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

/// Creates a connection pool to the given Postgres database.
///
/// # Errors
/// Returns `DbError::Sqlx` if the connection fails.
pub async fn create_pool(database_url: &str) -> Result<PgPool, DbError> {
    Ok(PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?)
}

/// Runs all pending database migrations.
///
/// # Errors
/// Returns `DbError::Migrate` if a migration fails.
pub async fn run_migrations(pool: &PgPool) -> Result<(), DbError> {
    sqlx::migrate!("../kassi-db/migrations").run(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = "../kassi-db/migrations")]
    async fn migrations_run_cleanly(_pool: PgPool) {}

    #[sqlx::test(migrations = "../kassi-db/migrations")]
    async fn migrations_are_idempotent(pool: PgPool) {
        run_migrations(&pool).await.unwrap();
    }
}
