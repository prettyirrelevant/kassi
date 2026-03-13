mod common;

use axum::http::{Request, StatusCode};
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::schema;
use tower::ServiceExt;

use common::{request_basic_auth, TestContext};

const USERNAME: &str = "kassi";
const ADMIN_PASSWORD: &str = "admin-secret";

mod relayers {
    use super::*;

    #[tokio::test]
    async fn returns_empty_list() {
        let ctx = TestContext::new().await;

        let (status, json) = request_basic_auth(
            &ctx.state,
            "GET",
            "/admin/relayers",
            USERNAME,
            ADMIN_PASSWORD,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_auth_returns_401() {
        let ctx = TestContext::new().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::get("/admin/relayers")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_password_returns_401() {
        let ctx = TestContext::new().await;

        let (status, _) = request_basic_auth(
            &ctx.state,
            "GET",
            "/admin/relayers",
            USERNAME,
            "wrong",
            None,
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn internal_password_does_not_work_for_admin() {
        let ctx = TestContext::new().await;

        let (status, _) = request_basic_auth(
            &ctx.state,
            "GET",
            "/admin/relayers",
            USERNAME,
            "internal-secret",
            None,
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}

mod queues {
    use super::*;

    #[tokio::test]
    async fn returns_stats_grouped_by_queue() {
        let ctx = TestContext::new().await;

        // seed some jobs
        let mut conn = ctx.state.db.get().await.unwrap();
        for queue in &["deposits", "sweeps", "webhooks"] {
            kassi_db::diesel::insert_into(schema::jobs::table)
                .values((
                    schema::jobs::queue.eq(queue),
                    schema::jobs::payload.eq(serde_json::json!({})),
                    schema::jobs::status.eq("pending"),
                    schema::jobs::max_attempts.eq(3),
                ))
                .execute(&mut conn)
                .await
                .unwrap();
        }

        // add a failed job
        kassi_db::diesel::insert_into(schema::jobs::table)
            .values((
                schema::jobs::queue.eq("sweeps"),
                schema::jobs::payload.eq(serde_json::json!({})),
                schema::jobs::status.eq("failed"),
                schema::jobs::max_attempts.eq(3),
            ))
            .execute(&mut conn)
            .await
            .unwrap();

        let (status, json) = request_basic_auth(
            &ctx.state,
            "GET",
            "/admin/queues",
            USERNAME,
            ADMIN_PASSWORD,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["deposits"]["pending"].as_i64().unwrap(), 1);
        assert_eq!(json["data"]["sweeps"]["pending"].as_i64().unwrap(), 1);
        assert_eq!(json["data"]["sweeps"]["failed"].as_i64().unwrap(), 1);
        assert_eq!(json["data"]["webhooks"]["pending"].as_i64().unwrap(), 1);
    }

    #[tokio::test]
    async fn empty_queues() {
        let ctx = TestContext::new().await;

        let (status, json) = request_basic_auth(
            &ctx.state,
            "GET",
            "/admin/queues",
            USERNAME,
            ADMIN_PASSWORD,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_auth_returns_401() {
        let ctx = TestContext::new().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::get("/admin/queues")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
