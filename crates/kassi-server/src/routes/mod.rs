mod health;

use axum::Router;

use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().merge(health::routes())
}
