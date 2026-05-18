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
    #[error("internal error: {0}")]
    Internal(String),
}

pub fn bad_request(message: &str) -> Result<Response> {
    let body = serde_json::json!({
        "ok": false,
        "error": message,
    });
    Response::from_json(&body).map(|r| r.with_status(400))
}

pub fn from_convert_error(err: ConvertError) -> Result<Response> {
    let status = match &err {
        ConvertError::UnsupportedInput(_) | ConvertError::UnsupportedOutput(_) => 400,
        ConvertError::PayloadTooLarge(_, _) => 413,
        ConvertError::Parse { .. } => 422,
        ConvertError::PdfNotConfigured => 503,
        ConvertError::PdfRender(_) | ConvertError::Internal(_) => 502,
    };
    let body = serde_json::json!({
        "ok": false,
        "error": err.to_string(),
    });
    Response::from_json(&body).map(|r| r.with_status(status))
}
