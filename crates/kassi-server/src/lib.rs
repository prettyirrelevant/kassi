pub mod config;
pub mod errors;
pub mod response;
mod routes;

use axum::http::Method;
use axum::Router;
use sqlx::PgPool;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

#[must_use]
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

#[cfg(test)]
mod tests {
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    mod health {
        use super::*;

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn returns_200_with_healthy_status(pool: PgPool) {
            let response = app(AppState { db: pool })
                .oneshot(
                    Request::get("/health")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

            assert_eq!(json["data"]["status"], "healthy");
        }
    }

    mod fallback {
        use super::*;

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn unknown_route_returns_404(pool: PgPool) {
            let response = app(AppState { db: pool })
                .oneshot(
                    Request::get("/nonexistent")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::NOT_FOUND);

            let body = response.into_body().collect().await.unwrap().to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

            assert_eq!(json["error"]["code"], "route_not_found");
        }
    }

    mod cors {
        use super::*;

        #[sqlx::test(migrations = "../kassi-db/migrations")]
        async fn preflight_returns_cors_headers(pool: PgPool) {
            let response = app(AppState { db: pool })
                .oneshot(
                    Request::options("/health")
                        .header("origin", "https://example.com")
                        .header("access-control-request-method", "GET")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert!(response
                .headers()
                .contains_key("access-control-allow-origin"));
            assert!(response
                .headers()
                .contains_key("access-control-allow-methods"));
        }
    }
}
