use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::io::Write;
use tempfile::NamedTempFile;
use tokio::process::Command;

use crate::error::ConvertError;

/// Render HTML to a PDF byte buffer by spawning a local headless Chromium
/// process. The binary path is configured via `CHROMIUM_BIN` (defaults to
/// `chromium`). The HTML is written to a temp file and Chromium's
/// `--print-to-pdf` flag emits the PDF to another temp file, which we then
/// read back.
///
/// `page_numbers` controls whether Chromium's built-in header/footer (page
/// numbers + document title) is included. Custom headers/footers are
/// rendered via CSS `position: fixed` elements in the document itself.
pub async fn render(
    chromium_bin: &str,
    html: &str,
    page_numbers: bool,
) -> Result<Vec<u8>, ConvertError> {
    let mut html_file = NamedTempFile::with_suffix(".html")
        .map_err(|e| ConvertError::Internal(format!("temp html: {e}")))?;
    html_file
        .write_all(html.as_bytes())
        .map_err(|e| ConvertError::Internal(format!("write html: {e}")))?;
    html_file
        .flush()
        .map_err(|e| ConvertError::Internal(format!("flush html: {e}")))?;
    let html_path = html_file.path().to_path_buf();

    let pdf_file = NamedTempFile::with_suffix(".pdf")
        .map_err(|e| ConvertError::Internal(format!("temp pdf: {e}")))?;
    let pdf_path = pdf_file.path().to_path_buf();
    // We need Chromium to be able to write to this path; drop the handle but
    // keep the path. The TempPath cleans up on drop.
    let pdf_path_handle = pdf_file.into_temp_path();

    let print_to_pdf = format!("--print-to-pdf={}", pdf_path.display());
    let file_url = format!("file://{}", html_path.display());
    let mut args: Vec<&str> = vec![
        "--headless=new",
        "--disable-gpu",
        "--no-sandbox",
        "--disable-dev-shm-usage",
        "--hide-scrollbars",
        "--run-all-compositor-stages-before-draw",
    ];
    if !page_numbers {
        args.push("--no-pdf-header-footer");
    }
    args.push(&print_to_pdf);
    args.push(&file_url);

    let output = Command::new(chromium_bin)
        .args(&args)
        .output()
        .await
        .map_err(|e| ConvertError::PdfRender(format!("spawn chromium: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ConvertError::PdfRender(format!(
            "chromium exited with {}: {}",
            output.status,
            truncate(&stderr, 500)
        )));
    }

    let bytes = tokio::fs::read(&pdf_path)
        .await
        .map_err(|e| ConvertError::PdfRender(format!("read pdf: {e}")))?;

    drop(pdf_path_handle);
    drop(html_file);

    if bytes.is_empty() {
        return Err(ConvertError::PdfRender("chromium produced empty PDF".into()));
    }
    Ok(bytes)
}

pub fn encode_base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
