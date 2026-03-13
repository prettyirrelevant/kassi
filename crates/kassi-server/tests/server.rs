mod common;

use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::test_state;

mod health {
    use super::*;

    #[tokio::test]
    async fn returns_200_with_healthy_status() {
        let response = kassi_server::app(test_state().await)
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
        let response = kassi_server::app(test_state().await)
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
        let response = kassi_server::app(test_state().await)
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
