pub mod config;
pub mod errors;
pub mod response;
mod routes;

use axum::http::Method;
use axum::Router;
use kassi_db::DbPool;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
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

#[cfg(test)]
mod tests {
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    async fn test_pool() -> DbPool {
        kassi_db::create_pool(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
            .await
            .expect("failed to create test pool")
    }

    mod health {
        use super::*;

        #[tokio::test]
        async fn returns_200_with_healthy_status() {
            let response = app(AppState {
                db: test_pool().await,
            })
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

        #[tokio::test]
        async fn unknown_route_returns_404() {
            let response = app(AppState {
                db: test_pool().await,
            })
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

        #[tokio::test]
        async fn preflight_returns_cors_headers() {
            let response = app(AppState {
                db: test_pool().await,
            })
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
