use std::collections::HashMap;

use axum::extract::State;
use axum::routing::get;
use axum::Router;
use serde::Serialize;

use crate::errors::ServerError;
use crate::extractors::AdminAuth;
use crate::response::ApiSuccess;
use crate::AppState;

#[derive(Serialize)]
struct RelayerInfo {
    network_id: String,
    address: String,
    balance: String,
    unit: String,
}

#[derive(Default, Serialize)]
struct QueueStats {
    pending: i64,
    running: i64,
    failed: i64,
    dead: i64,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/relayers", get(list_relayers))
        .route("/admin/queues", get(queue_stats))
}

async fn list_relayers(_auth: AdminAuth) -> Result<ApiSuccess<Vec<RelayerInfo>>, ServerError> {
    // relayer balance fetching requires RPC clients, which are wired in the workers phase.
    // for now, return an empty list.
    Ok(ApiSuccess { data: vec![] })
}

async fn queue_stats(
    State(state): State<AppState>,
    _auth: AdminAuth,
) -> Result<ApiSuccess<HashMap<String, QueueStats>>, ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let rows = kassi_db::queries::job_counts_by_queue_and_status(&mut conn).await?;

    let mut queues: HashMap<String, QueueStats> = HashMap::new();
    for (queue, status, count) in rows {
        let entry = queues.entry(queue).or_default();
        match status.as_str() {
            "pending" => entry.pending = count,
            "running" => entry.running = count,
            "failed" => entry.failed = count,
            "dead" => entry.dead = count,
            _ => {}
        }
    }

    Ok(ApiSuccess { data: queues })
}
