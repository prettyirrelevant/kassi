mod common;

use axum::http::{Request, StatusCode};
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::schema;
use kassi_server::AppState;
use tower::ServiceExt;

use common::{authenticate, request, TestContext};

async fn seed_webhook_delivery(
    state: &AppState,
    id: &str,
    merchant_id: &str,
    event_type: &str,
    status: &str,
) {
    let mut conn = state.db.get().await.unwrap();
    kassi_db::diesel::insert_into(schema::webhook_deliveries::table)
        .values((
            schema::webhook_deliveries::id.eq(id),
            schema::webhook_deliveries::merchant_id.eq(merchant_id),
            schema::webhook_deliveries::event_type.eq(event_type),
            schema::webhook_deliveries::reference_id.eq("le_test123"),
            schema::webhook_deliveries::url.eq("https://example.com/webhook"),
            schema::webhook_deliveries::payload.eq(serde_json::json!({"test": true})),
            schema::webhook_deliveries::status.eq(status),
            schema::webhook_deliveries::attempts.eq(1),
        ))
        .execute(&mut conn)
        .await
        .unwrap();
}

mod list {
    use super::*;

    #[tokio::test]
    async fn list_webhook_deliveries() {
        let ctx = TestContext::new().await;
        let (token, merchant_id) = authenticate(&ctx.state).await;

        seed_webhook_delivery(
            &ctx.state,
            "whd_test1",
            &merchant_id,
            "deposit.confirmed",
            "sent",
        )
        .await;
        seed_webhook_delivery(
            &ctx.state,
            "whd_test2",
            &merchant_id,
            "payment_intent.confirmed",
            "failed",
        )
        .await;

        let (status, json) = request(&ctx.state, "GET", "/webhooks", &token, None).await;

        assert_eq!(status, StatusCode::OK);

        let data = json["data"].as_array().unwrap();
        assert_eq!(data.len(), 2);

        // should be ordered by created_at desc (most recent first)
        for delivery in data {
            assert!(delivery["id"].as_str().unwrap().starts_with("whd_"));
            assert_eq!(delivery["merchant_id"].as_str().unwrap(), merchant_id);
            assert!(delivery["event_type"].as_str().is_some());
            assert!(delivery["url"].as_str().is_some());
            assert!(delivery["payload"].is_object());
            assert!(delivery["status"].as_str().is_some());
            assert!(delivery["created_at"].as_str().is_some());
        }
    }

    #[tokio::test]
    async fn list_webhooks_scoped_to_merchant() {
        let ctx = TestContext::new().await;
        let (token_a, merchant_a) = authenticate(&ctx.state).await;
        let (token_b, _) = authenticate(&ctx.state).await;

        seed_webhook_delivery(
            &ctx.state,
            "whd_merch_a",
            &merchant_a,
            "deposit.confirmed",
            "sent",
        )
        .await;

        // merchant B sees nothing
        let (status, json) = request(&ctx.state, "GET", "/webhooks", &token_b, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_array().unwrap().is_empty());

        // merchant A sees their delivery
        let (status, json) = request(&ctx.state, "GET", "/webhooks", &token_a, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn list_webhooks_empty() {
        let ctx = TestContext::new().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) = request(&ctx.state, "GET", "/webhooks", &token, None).await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_array().unwrap().is_empty());
        assert!(json["meta"]["next_page"].is_null());
    }

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let ctx = TestContext::new().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::get("/webhooks")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

mod get {
    use super::*;

    #[tokio::test]
    async fn get_webhook_delivery_by_id() {
        let ctx = TestContext::new().await;
        let (token, merchant_id) = authenticate(&ctx.state).await;

        seed_webhook_delivery(
            &ctx.state,
            "whd_detail1",
            &merchant_id,
            "deposit.confirmed",
            "sent",
        )
        .await;

        let (status, json) =
            request(&ctx.state, "GET", "/webhooks/whd_detail1", &token, None).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["id"].as_str().unwrap(), "whd_detail1");
        assert_eq!(
            json["data"]["event_type"].as_str().unwrap(),
            "deposit.confirmed"
        );
        assert_eq!(json["data"]["status"].as_str().unwrap(), "sent");
    }

    #[tokio::test]
    async fn get_nonexistent_returns_404() {
        let ctx = TestContext::new().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) =
            request(&ctx.state, "GET", "/webhooks/whd_nonexistent", &token, None).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            json["error"]["code"].as_str().unwrap(),
            "resource_not_found"
        );
    }

    #[tokio::test]
    async fn get_other_merchants_delivery_returns_404() {
        let ctx = TestContext::new().await;
        let (_, merchant_a) = authenticate(&ctx.state).await;
        let (token_b, _) = authenticate(&ctx.state).await;

        seed_webhook_delivery(
            &ctx.state,
            "whd_private",
            &merchant_a,
            "deposit.confirmed",
            "sent",
        )
        .await;

        // merchant B cannot see merchant A's delivery
        let (status, json) =
            request(&ctx.state, "GET", "/webhooks/whd_private", &token_b, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            json["error"]["code"].as_str().unwrap(),
            "resource_not_found"
        );
    }
}

mod retry {
    use super::*;

    #[tokio::test]
    async fn retry_enqueues_a_new_webhook_job() {
        let ctx = TestContext::new().await;
        let (token, merchant_id) = authenticate(&ctx.state).await;

        seed_webhook_delivery(
            &ctx.state,
            "whd_retry1",
            &merchant_id,
            "deposit.confirmed",
            "failed",
        )
        .await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/webhooks/whd_retry1/retry",
            &token,
            Some(serde_json::json!({})),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["id"].as_str().unwrap(), "whd_retry1");

        // verify a webhook job was enqueued
        let mut conn = ctx.state.db.get().await.unwrap();
        let jobs: Vec<(String, serde_json::Value)> = schema::jobs::table
            .filter(schema::jobs::queue.eq("webhooks"))
            .filter(schema::jobs::status.eq("pending"))
            .select((schema::jobs::queue, schema::jobs::payload))
            .load::<(String, serde_json::Value)>(&mut conn)
            .await
            .unwrap();

        assert_eq!(jobs.len(), 1);
        assert_eq!(
            jobs[0].1["webhook_delivery_id"].as_str().unwrap(),
            "whd_retry1"
        );
        assert_eq!(
            jobs[0].1["event_type"].as_str().unwrap(),
            "deposit.confirmed"
        );
    }

    #[tokio::test]
    async fn retry_nonexistent_returns_404() {
        let ctx = TestContext::new().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/webhooks/whd_nonexistent/retry",
            &token,
            Some(serde_json::json!({})),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            json["error"]["code"].as_str().unwrap(),
            "resource_not_found"
        );
    }
}
