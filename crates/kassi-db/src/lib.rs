use sqlx::postgres::{PgPool, PgPoolOptions};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

pub async fn create_pool(database_url: &str) -> Result<PgPool, DbError> {
    Ok(PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?)
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), DbError> {
    sqlx::migrate!("../kassi-db/migrations")
        .run(pool)
        .await?;
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
