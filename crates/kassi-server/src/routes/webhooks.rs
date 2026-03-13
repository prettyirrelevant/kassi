use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use kassi_db::models::{NewJob, WebhookDelivery};
use serde::{Deserialize, Serialize};

use super::shared;
use crate::errors::ServerError;
use crate::extractors::{AnyAuth, SessionAuth};
use crate::response::{ApiList, ApiSuccess, ListMeta};
use crate::AppState;

#[derive(Deserialize)]
struct ListParams {
    page: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct WebhookDeliveryResponse {
    id: String,
    merchant_id: String,
    event_type: String,
    reference_id: String,
    url: String,
    payload: serde_json::Value,
    status: String,
    attempts: i32,
    last_attempt_at: Option<DateTime<Utc>>,
    response_code: Option<i16>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn to_response(d: WebhookDelivery) -> WebhookDeliveryResponse {
    WebhookDeliveryResponse {
        id: d.id,
        merchant_id: d.merchant_id,
        event_type: d.event_type,
        reference_id: d.reference_id,
        url: d.url,
        payload: d.payload,
        status: d.status,
        attempts: d.attempts,
        last_attempt_at: d.last_attempt_at,
        response_code: d.response_code,
        created_at: d.created_at,
        updated_at: d.updated_at,
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/webhooks", get(list_webhooks))
        .route("/webhooks/{id}", get(get_webhook))
        .route("/webhooks/{id}/retry", post(retry_webhook))
}

async fn list_webhooks(
    State(state): State<AppState>,
    auth: AnyAuth,
    Query(params): Query<ListParams>,
) -> Result<ApiList<WebhookDeliveryResponse>, ServerError> {
    let limit = params.limit.unwrap_or(20).min(100);

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let cursor = params
        .page
        .as_ref()
        .map(|p| shared::decode_cursor(p))
        .transpose()?;
    let cursor_ref = cursor.as_ref().map(|(t, id)| (t, id.as_str()));

    let mut rows = kassi_db::queries::list_webhook_deliveries(
        &mut conn,
        &auth.merchant_id,
        i64::try_from(limit + 1).unwrap_or(i64::MAX),
        cursor_ref,
    )
    .await?;

    let has_next = rows.len() > limit;
    if has_next {
        rows.truncate(limit);
    }

    let next_page = if has_next {
        rows.last()
            .map(|d| shared::encode_cursor(&d.created_at, &d.id))
    } else {
        None
    };

    let data = rows.into_iter().map(to_response).collect();

    Ok(ApiList {
        data,
        meta: ListMeta {
            next_page,
            previous_page: None,
        },
    })
}

async fn get_webhook(
    State(state): State<AppState>,
    auth: AnyAuth,
    Path(id): Path<String>,
) -> Result<ApiSuccess<WebhookDeliveryResponse>, ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let delivery = kassi_db::queries::get_webhook_delivery(&mut conn, &id, &auth.merchant_id)
        .await?
        .ok_or_else(|| ServerError::NotFound {
            entity: "webhook_delivery",
            id: id.clone(),
        })?;

    Ok(ApiSuccess {
        data: to_response(delivery),
    })
}

async fn retry_webhook(
    State(state): State<AppState>,
    auth: SessionAuth,
    Path(id): Path<String>,
) -> Result<ApiSuccess<WebhookDeliveryResponse>, ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let delivery = kassi_db::queries::get_webhook_delivery(&mut conn, &id, &auth.merchant_id)
        .await?
        .ok_or_else(|| ServerError::NotFound {
            entity: "webhook_delivery",
            id: id.clone(),
        })?;

    // enqueue a webhook retry job
    let job_payload = serde_json::json!({
        "webhook_delivery_id": delivery.id,
        "merchant_id": delivery.merchant_id,
        "event_type": delivery.event_type,
        "url": delivery.url,
        "payload": delivery.payload,
    });

    kassi_db::queries::insert_job(
        &mut conn,
        NewJob {
            queue: "webhooks",
            payload: job_payload,
            max_attempts: 10,
            scheduled_at: None,
        },
    )
    .await?;

    Ok(ApiSuccess {
        data: to_response(delivery),
    })
}
