//! Batch conversion: accept up to N `ConvertRequest`s and return either
//! a JSON array of results or a zip of PDFs/HTML files. Optionally POSTs
//! the JSON result to a caller-supplied webhook URL, signed with HMAC-
//! SHA-256 so receivers can verify it came from us.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::io::Write as _;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::convert::{self, ConvertRequest, ConvertResponse};
use crate::db::AppState;
use crate::error::ConvertError;

const MAX_ITEMS: usize = 50;

#[derive(Debug, Deserialize)]
pub struct BatchRequest {
    pub items: Vec<ConvertRequest>,
    #[serde(default)]
    pub webhook: Option<WebhookConfig>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    /// Shared secret. The server signs the JSON payload with HMAC-SHA-256
    /// and sends the hex digest in the `X-UDC-Signature` header.
    #[serde(default)]
    pub secret: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchItemResult {
    pub index: usize,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ConvertResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchResponse {
    pub ok: bool,
    pub items: Vec<BatchItemResult>,
    pub webhook_delivered: Option<bool>,
}

pub fn validate(req: &BatchRequest) -> Result<(), ConvertError> {
    if req.items.is_empty() {
        return Err(ConvertError::BadRequest("batch: items must be non-empty".into()));
    }
    if req.items.len() > MAX_ITEMS {
        return Err(ConvertError::BadRequest(format!(
            "batch: too many items ({} > {})",
            req.items.len(),
            MAX_ITEMS
        )));
    }
    if let Some(hook) = &req.webhook {
        if !(hook.url.starts_with("https://") || hook.url.starts_with("http://")) {
            return Err(ConvertError::BadRequest(
                "webhook.url must be http(s)".into(),
            ));
        }
        if hook.url.len() > 2048 {
            return Err(ConvertError::BadRequest("webhook.url is too long".into()));
        }
    }
    Ok(())
}

pub async fn run(
    req: &BatchRequest,
    state: &AppState,
    user: &crate::db::User,
) -> Result<BatchResponse, ConvertError> {
    let mut items = Vec::with_capacity(req.items.len());
    for (index, item) in req.items.iter().enumerate() {
        match convert::run(item, state, Some(user)).await {
            Ok(result) => items.push(BatchItemResult {
                index,
                ok: true,
                result: Some(result),
                error: None,
            }),
            Err(e) => items.push(BatchItemResult {
                index,
                ok: false,
                result: None,
                error: Some(e.to_string()),
            }),
        }
    }
    let response = BatchResponse {
        ok: items.iter().all(|i| i.ok),
        items,
        webhook_delivered: None,
    };

    let webhook_delivered = match &req.webhook {
        Some(hook) => Some(deliver_webhook(hook, &response).await),
        None => None,
    };

    Ok(BatchResponse {
        webhook_delivered,
        ..response
    })
}

/// Pack the batch results into a zip archive. PDF outputs are stored as
/// binary `.pdf` files; HTML outputs are stored as `.html`. Errors are
/// recorded as `.error.txt` for that index.
pub fn to_zip(response: &BatchResponse) -> Result<Vec<u8>, ConvertError> {
    let buf = std::io::Cursor::new(Vec::<u8>::new());
    let mut zip = ZipWriter::new(buf);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for item in &response.items {
        if let Some(err) = &item.error {
            zip.start_file(format!("{:03}.error.txt", item.index), opts)
                .map_err(zip_err)?;
            zip.write_all(err.as_bytes()).map_err(zip_err_io)?;
            continue;
        }
        let Some(result) = &item.result else { continue };
        if let Some(pdf_b64) = &result.pdf_base64 {
            let bytes = STANDARD.decode(pdf_b64).map_err(|e| {
                ConvertError::Internal(format!("decode pdf base64: {e}"))
            })?;
            zip.start_file(format!("{:03}.pdf", item.index), opts)
                .map_err(zip_err)?;
            zip.write_all(&bytes).map_err(zip_err_io)?;
        } else if let Some(html) = &result.content {
            zip.start_file(format!("{:03}.html", item.index), opts)
                .map_err(zip_err)?;
            zip.write_all(html.as_bytes()).map_err(zip_err_io)?;
        }
    }
    let cursor = zip.finish().map_err(zip_err)?;
    Ok(cursor.into_inner())
}

async fn deliver_webhook(config: &WebhookConfig, response: &BatchResponse) -> bool {
    let body = match serde_json::to_vec(response) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let signature = config.secret.as_deref().map(|s| sign(s, &body));
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut req = client
        .post(&config.url)
        .header("content-type", "application/json")
        .body(body);
    if let Some(sig) = signature {
        req = req.header("x-udc-signature", sig);
    }
    match req.send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

fn sign(secret: &str, body: &[u8]) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC supports any key length");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

fn zip_err(e: zip::result::ZipError) -> ConvertError {
    ConvertError::Internal(format!("zip: {e}"))
}

fn zip_err_io(e: std::io::Error) -> ConvertError {
    ConvertError::Internal(format!("zip io: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_is_deterministic_and_hex() {
        let a = sign("secret", b"payload");
        let b = sign("secret", b"payload");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn validate_rejects_empty_batch() {
        let req = BatchRequest {
            items: vec![],
            webhook: None,
        };
        assert!(validate(&req).is_err());
    }
}
