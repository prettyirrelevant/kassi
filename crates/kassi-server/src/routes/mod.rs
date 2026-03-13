pub mod auth;
mod deposit_addresses;
mod health;
mod merchants;
mod settlement_destinations;

use axum::Router;

use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(merchants::routes())
        .merge(settlement_destinations::routes())
        .merge(deposit_addresses::routes())
}
