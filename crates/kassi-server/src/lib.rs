pub mod config;
pub mod errors;
pub mod extractors;
pub mod response;
pub mod routes;

use std::sync::Arc;

use axum::http::Method;
use axum::Router;
use kassi_db::DbPool;
use kassi_signer::InfisicalKms;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub config: config::Config,
    pub kms: Option<Arc<InfisicalKms>>,
}

pub fn app(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(Any);

    Router::new()
        .merge(routes::routes())
        .fallback(|| async { errors::ServerError::RouteNotFound })
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
