pub mod auth;
mod deposit_addresses;
mod health;
mod merchants;
mod payment_intents;
mod refunds;
mod settlement_destinations;
mod shared;

use axum::Router;

use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(merchants::routes())
        .merge(settlement_destinations::routes())
        .merge(deposit_addresses::routes())
        .merge(payment_intents::routes())
        .merge(refunds::routes())
}
