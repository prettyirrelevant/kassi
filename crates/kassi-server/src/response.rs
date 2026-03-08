use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiSuccess<T> {
    pub data: T,
}

#[derive(Debug, Serialize)]
pub struct ApiList<T> {
    pub data: Vec<T>,
    pub meta: ListMeta,
}

#[derive(Debug, Serialize)]
pub struct ListMeta {
    pub next_page: Option<String>,
    pub previous_page: Option<String>,
}

impl<T: Serialize> IntoResponse for ApiSuccess<T> {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

impl<T: Serialize> IntoResponse for ApiList<T> {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

impl<T: Serialize> ApiSuccess<T> {
    pub fn created(data: T) -> Response {
        (StatusCode::CREATED, Json(ApiSuccess { data })).into_response()
    }
}
