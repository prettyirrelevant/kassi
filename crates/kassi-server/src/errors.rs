use std::borrow::Cow;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ErrorBody<'a> {
    error: ErrorPayload<'a>,
}

#[derive(Debug, Serialize)]
struct ErrorPayload<'a> {
    code: &'static str,
    message: Cow<'a, str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Vec<ValidationDetail>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationDetail {
    pub field: String,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("the requested route does not exist.")]
    RouteNotFound,

    #[error("missing or invalid authentication credentials.")]
    AuthenticationRequired,

    #[error("you do not have permission to access this resource.")]
    Forbidden,

    #[error("no {entity} found with id '{id}'.")]
    NotFound { entity: &'static str, id: String },

    #[error("{0}")]
    Conflict(String),

    #[error("request validation failed.")]
    ValidationFailed(Vec<ValidationDetail>),

    #[error("{0}")]
    BadRequest(String),

    #[error("an unexpected server error occurred.")]
    Internal(#[from] kassi_db::DbError),
}

impl ServerError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::RouteNotFound => (StatusCode::NOT_FOUND, "route_not_found"),
            Self::AuthenticationRequired => (StatusCode::UNAUTHORIZED, "authentication_required"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound { .. } => (StatusCode::NOT_FOUND, "resource_not_found"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Self::ValidationFailed(_) => (StatusCode::BAD_REQUEST, "validation_failed"),
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        }
    }
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();

        if let Self::Internal(ref e) = self {
            tracing::error!("internal error: {e}");
        }

        let (message, details) = match self {
            Self::RouteNotFound => (Cow::Borrowed("the requested route does not exist."), None),
            Self::AuthenticationRequired => (
                Cow::Borrowed("missing or invalid authentication credentials."),
                None,
            ),
            Self::Forbidden => (
                Cow::Borrowed("you do not have permission to access this resource."),
                None,
            ),
            Self::NotFound { entity, id } => (
                Cow::Owned(format!("no {entity} found with id '{id}'.")),
                None,
            ),
            Self::Conflict(msg) | Self::BadRequest(msg) => (Cow::Owned(msg), None),
            Self::ValidationFailed(details) => {
                (Cow::Borrowed("request validation failed."), Some(details))
            }
            Self::Internal(_) => (Cow::Borrowed("an unexpected server error occurred."), None),
        };

        let body = ErrorBody {
            error: ErrorPayload {
                code,
                message,
                details,
            },
        };

        (status, Json(body)).into_response()
    }
}
