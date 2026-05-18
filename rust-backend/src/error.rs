use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("unsupported input type: {0}")]
    UnsupportedInput(String),
    #[error("unsupported output type: {0}")]
    UnsupportedOutput(String),
    #[error("payload too large: {0} bytes (max {1})")]
    PayloadTooLarge(usize, usize),
    #[error("parse error ({format}): {message}")]
    Parse {
        format: &'static str,
        message: String,
    },
    #[error("PDF rendering failed: {0}")]
    PdfRender(String),
    #[error("invalid request: {0}")]
    BadRequest(String),
    #[error("authentication required")]
    Unauthorized,
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("premium subscription required to use this feature")]
    PremiumRequired,
    #[error("database error: {0}")]
    Database(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<sqlx::Error> for ConvertError {
    fn from(e: sqlx::Error) -> Self {
        ConvertError::Database(e.to_string())
    }
}

impl From<std::io::Error> for ConvertError {
    fn from(e: std::io::Error) -> Self {
        ConvertError::Internal(format!("io: {e}"))
    }
}

impl ConvertError {
    pub fn status(&self) -> StatusCode {
        match self {
            ConvertError::UnsupportedInput(_)
            | ConvertError::UnsupportedOutput(_)
            | ConvertError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ConvertError::Unauthorized => StatusCode::UNAUTHORIZED,
            ConvertError::Forbidden(_) | ConvertError::PremiumRequired => StatusCode::FORBIDDEN,
            ConvertError::NotFound => StatusCode::NOT_FOUND,
            ConvertError::Conflict(_) => StatusCode::CONFLICT,
            ConvertError::PayloadTooLarge(_, _) => StatusCode::PAYLOAD_TOO_LARGE,
            ConvertError::Parse { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            ConvertError::PdfRender(_)
            | ConvertError::Database(_)
            | ConvertError::Internal(_) => StatusCode::BAD_GATEWAY,
        }
    }
}

impl IntoResponse for ConvertError {
    fn into_response(self) -> Response {
        let status = self.status();
        let message = self.to_string();
        if status.is_server_error() {
            tracing::error!(error = %message, "request failed");
        } else {
            tracing::debug!(error = %message, "request rejected");
        }
        (status, Json(json!({ "ok": false, "error": message }))).into_response()
    }
}
