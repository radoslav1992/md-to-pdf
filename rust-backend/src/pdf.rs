use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use worker::{Env, Fetch, Headers, Method, Request, RequestInit};

use crate::error::ConvertError;

/// Dispatch an outbound HTTPS request to an external headless-browser service
/// (Browserless, Puppeteer-compatible) to render HTML to PDF. Headless
/// browsers cannot execute inside a Workers isolate, so this hop is required.
pub async fn render(html: &str, env: &Env) -> Result<Vec<u8>, ConvertError> {
    let url = env
        .var("PDF_RENDER_URL")
        .map_err(|_| ConvertError::PdfNotConfigured)?
        .to_string();

    let token = env.secret("PDF_RENDER_TOKEN").ok().map(|t| t.to_string());

    let payload = RenderPayload {
        html,
        options: RenderOptions {
            print_background: true,
            format: "A4",
            margin: PageMargin {
                top: "0.4in",
                right: "0.4in",
                bottom: "0.4in",
                left: "0.4in",
            },
        },
    };

    let body = serde_json::to_string(&payload)
        .map_err(|e| ConvertError::Internal(format!("payload encode: {e}")))?;

    let mut headers = Headers::new();
    headers
        .set("content-type", "application/json")
        .map_err(|e| ConvertError::Internal(e.to_string()))?;
    headers
        .set("accept", "application/pdf")
        .map_err(|e| ConvertError::Internal(e.to_string()))?;
    if let Some(t) = token {
        headers
            .set("authorization", &format!("Bearer {t}"))
            .map_err(|e| ConvertError::Internal(e.to_string()))?;
    }

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(body.into()));

    let request = Request::new_with_init(&url, &init)
        .map_err(|e| ConvertError::Internal(format!("build request: {e}")))?;

    let mut response = Fetch::Request(request)
        .send()
        .await
        .map_err(|e| ConvertError::PdfRender(format!("upstream fetch failed: {e}")))?;

    let status = response.status_code();
    if !(200..300).contains(&status) {
        let text = response.text().await.unwrap_or_default();
        return Err(ConvertError::PdfRender(format!(
            "upstream returned {status}: {text}"
        )));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| ConvertError::PdfRender(format!("read body: {e}")))?;
    Ok(bytes)
}

pub fn encode_base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

#[derive(Serialize)]
struct RenderPayload<'a> {
    html: &'a str,
    options: RenderOptions,
}

#[derive(Serialize)]
struct RenderOptions {
    #[serde(rename = "printBackground")]
    print_background: bool,
    format: &'static str,
    margin: PageMargin,
}

#[derive(Serialize)]
struct PageMargin {
    top: &'static str,
    right: &'static str,
    bottom: &'static str,
    left: &'static str,
}
