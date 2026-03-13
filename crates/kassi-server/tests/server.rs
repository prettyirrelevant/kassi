mod common;

use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::TestContext;

mod health {
    use super::*;

    #[tokio::test]
    async fn returns_200_with_healthy_status() {
        let ctx = TestContext::new().await;
        let response = kassi_server::app(ctx.state.clone())
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
        let ctx = TestContext::new().await;
        let response = kassi_server::app(ctx.state.clone())
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
        let ctx = TestContext::new().await;
        let response = kassi_server::app(ctx.state.clone())
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
