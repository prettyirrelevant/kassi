mod admin;
mod assets;
pub mod auth;
mod deposit_addresses;
mod health;
mod internal;
mod merchants;
mod payment_intents;
mod prices;
mod refunds;
mod settlement_destinations;
mod shared;
mod webhooks;

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
        .merge(assets::routes())
        .merge(prices::routes())
        .merge(webhooks::routes())
        .merge(internal::routes())
        .merge(admin::routes())
}
