pub mod config;
pub mod errors;
pub mod response;
mod routes;

use axum::Router;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::routes())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
