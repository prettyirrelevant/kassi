pub mod auth;
mod health;
mod merchants;

use axum::Router;

use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(merchants::routes())
}
