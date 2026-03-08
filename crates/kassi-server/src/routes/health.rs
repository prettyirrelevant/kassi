use axum::routing::get;
use axum::Router;
use serde::Serialize;

use crate::response::ApiSuccess;
use crate::AppState;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health() -> ApiSuccess<HealthResponse> {
    ApiSuccess {
        data: HealthResponse { status: "healthy" },
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/health", get(health))
}
