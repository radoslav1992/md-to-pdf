use thiserror::Error;
use worker::{Response, Result};

#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("unsupported input type: {0}")]
    UnsupportedInput(String),
    #[error("unsupported output type: {0}")]
    UnsupportedOutput(String),
    #[error("payload too large: {0} bytes (max {1})")]
    PayloadTooLarge(usize, usize),
    #[error("parse error ({format}): {message}")]
    Parse { format: &'static str, message: String },
    #[error("PDF rendering failed: {0}")]
    PdfRender(String),
    #[error("PDF render service not configured")]
    PdfNotConfigured,
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

pub fn bad_request(message: &str) -> Result<Response> {
    error_response(400, message)
}

pub fn from_convert_error(err: ConvertError) -> Result<Response> {
    let status = match &err {
        ConvertError::UnsupportedInput(_)
        | ConvertError::UnsupportedOutput(_)
        | ConvertError::BadRequest(_) => 400,
        ConvertError::Unauthorized => 401,
        ConvertError::Forbidden(_) | ConvertError::PremiumRequired => 403,
        ConvertError::NotFound => 404,
        ConvertError::Conflict(_) => 409,
        ConvertError::PayloadTooLarge(_, _) => 413,
        ConvertError::Parse { .. } => 422,
        ConvertError::PdfNotConfigured => 503,
        ConvertError::PdfRender(_) | ConvertError::Database(_) | ConvertError::Internal(_) => 502,
    };
    error_response(status, &err.to_string())
}

fn error_response(status: u16, message: &str) -> Result<Response> {
    let body = serde_json::json!({
        "ok": false,
        "error": message,
    });
    Response::from_json(&body).map(|r| r.with_status(status))
}
