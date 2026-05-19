//! Reverse-direction extraction: PDF → Markdown.
//!
//! `pdftotext` (from poppler-utils) handles text-based PDFs cheaply and
//! accurately. When the PDF is image-only (typical of scans), pdftotext
//! returns near-empty output; in that case we fall back to `pdftoppm` +
//! `tesseract` to OCR each page. Both binaries are bundled in the runtime
//! image — see the Dockerfile.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tempfile::TempDir;
use tokio::process::Command;

use crate::error::ConvertError;

const MAX_PDF_BYTES: usize = 16 * 1024 * 1024;
const MIN_CHARS_PER_PAGE: usize = 80;

#[derive(Debug, Deserialize)]
pub struct ExtractRequest {
    /// Base64-encoded PDF bytes.
    pub pdf_base64: String,
    /// "auto" (default) tries pdftotext first and OCRs only if needed,
    /// "force" always OCRs, "off" never OCRs.
    #[serde(default = "default_ocr")]
    pub ocr: String,
}

fn default_ocr() -> String {
    "auto".to_string()
}

#[derive(Debug, Serialize)]
pub struct ExtractResponse {
    pub ok: bool,
    pub method: String,
    pub markdown: String,
    pub page_count: Option<u32>,
}

pub async fn run(req: &ExtractRequest) -> Result<ExtractResponse, ConvertError> {
    let bytes = STANDARD
        .decode(req.pdf_base64.as_bytes())
        .map_err(|e| ConvertError::BadRequest(format!("invalid pdf_base64: {e}")))?;
    if bytes.len() > MAX_PDF_BYTES {
        return Err(ConvertError::PayloadTooLarge(bytes.len(), MAX_PDF_BYTES));
    }
    if !bytes.starts_with(b"%PDF") {
        return Err(ConvertError::BadRequest("not a PDF file".into()));
    }

    let tmp = TempDir::new().map_err(|e| ConvertError::Internal(format!("tempdir: {e}")))?;
    let pdf_path = tmp.path().join("input.pdf");
    tokio::fs::write(&pdf_path, &bytes)
        .await
        .map_err(|e| ConvertError::Internal(format!("write pdf: {e}")))?;

    let page_count = page_count(&pdf_path).await.ok();
    let mode = req.ocr.to_ascii_lowercase();

    let text_attempt = if mode == "force" {
        String::new()
    } else {
        run_pdftotext(&pdf_path).await.unwrap_or_default()
    };

    let pages = page_count.unwrap_or(1) as usize;
    let want_ocr = match mode.as_str() {
        "force" => true,
        "off" => false,
        _ => text_attempt.len() < pages.saturating_mul(MIN_CHARS_PER_PAGE),
    };

    if !want_ocr {
        return Ok(ExtractResponse {
            ok: true,
            method: "pdftotext".to_string(),
            markdown: text_to_markdown(&text_attempt),
            page_count,
        });
    }

    let ocr_text = run_ocr(&pdf_path, tmp.path()).await?;
    Ok(ExtractResponse {
        ok: true,
        method: "ocr".to_string(),
        markdown: text_to_markdown(&ocr_text),
        page_count,
    })
}

async fn page_count(pdf_path: &Path) -> Result<u32, ConvertError> {
    let output = Command::new("pdfinfo")
        .arg(pdf_path)
        .output()
        .await
        .map_err(|e| ConvertError::Internal(format!("pdfinfo: {e}")))?;
    if !output.status.success() {
        return Err(ConvertError::Internal("pdfinfo failed".into()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Pages:") {
            if let Ok(n) = rest.trim().parse::<u32>() {
                return Ok(n);
            }
        }
    }
    Err(ConvertError::Internal("page count not found".into()))
}

async fn run_pdftotext(pdf_path: &Path) -> Result<String, ConvertError> {
    let output = Command::new("pdftotext")
        .args(["-layout", "-enc", "UTF-8"])
        .arg(pdf_path)
        .arg("-")
        .output()
        .await
        .map_err(|e| ConvertError::Internal(format!("pdftotext: {e}")))?;
    if !output.status.success() {
        return Err(ConvertError::Internal(format!(
            "pdftotext exited with {}",
            output.status
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn run_ocr(pdf_path: &Path, workdir: &Path) -> Result<String, ConvertError> {
    // Rasterize each page to a PNG with pdftoppm.
    let prefix = workdir.join("page");
    let pdftoppm = Command::new("pdftoppm")
        .args(["-r", "200", "-png"])
        .arg(pdf_path)
        .arg(&prefix)
        .output()
        .await
        .map_err(|e| ConvertError::Internal(format!("pdftoppm: {e}")))?;
    if !pdftoppm.status.success() {
        return Err(ConvertError::Internal(format!(
            "pdftoppm failed: {}",
            String::from_utf8_lossy(&pdftoppm.stderr)
        )));
    }

    let mut pages: Vec<std::path::PathBuf> = Vec::new();
    let mut rd = tokio::fs::read_dir(workdir)
        .await
        .map_err(|e| ConvertError::Internal(format!("readdir: {e}")))?;
    while let Some(entry) = rd
        .next_entry()
        .await
        .map_err(|e| ConvertError::Internal(format!("readdir next: {e}")))?
    {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("page-") && n.ends_with(".png"))
        {
            pages.push(path);
        }
    }
    pages.sort();

    let mut out = String::new();
    for page in &pages {
        let result = Command::new("tesseract")
            .arg(page)
            .arg("-")
            .args(["-l", "eng"])
            .output()
            .await
            .map_err(|e| ConvertError::Internal(format!("tesseract: {e}")))?;
        if !result.status.success() {
            return Err(ConvertError::Internal(format!(
                "tesseract failed on page {}: {}",
                page.display(),
                String::from_utf8_lossy(&result.stderr)
            )));
        }
        if !out.is_empty() {
            out.push_str("\n\n---\n\n");
        }
        out.push_str(&String::from_utf8_lossy(&result.stdout));
    }
    Ok(out)
}

/// Very light text → markdown shaping: keep paragraphs (double newlines),
/// collapse intra-paragraph single newlines into spaces, and trim noise.
pub fn text_to_markdown(text: &str) -> String {
    let mut paragraphs = Vec::new();
    for raw in text.split("\n\n") {
        let para = raw
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !para.is_empty() {
            paragraphs.push(para);
        }
    }
    paragraphs.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_to_markdown_collapses_within_paragraph() {
        let out = text_to_markdown("Hello\nworld\n\nNext\npara");
        assert_eq!(out, "Hello world\n\nNext para");
    }

    #[test]
    fn text_to_markdown_drops_empty_lines() {
        let out = text_to_markdown("\n\n  \nHi\n\n");
        assert_eq!(out, "Hi");
    }
}
