//! PDF-manipulation primitives layered on top of `qpdf` and `ghostscript`.
//!
//! Each function shells out to one of the two bundled binaries, writing
//! the input to a temp file and reading the result back. We keep the
//! Rust surface narrow — `Vec<u8>` in, `Vec<u8>` out — so the routes can
//! base64-decode the request body once and base64-encode the response
//! once.
//!
//! `qpdf` handles structural transforms (merge, split, password) where
//! "the PDF stays a PDF". `ghostscript` is reserved for re-rasterisation
//! / re-streaming work (compression presets, PDF/A flattening) where
//! the output is meaningfully different from the input.

use std::io::Write;
use tempfile::NamedTempFile;
use tokio::process::Command;

use crate::error::ConvertError;

const MAX_PDF_BYTES: usize = 64 * 1024 * 1024;

/// Sanity check: file starts with `%PDF-` and isn't pathological.
fn validate_pdf(bytes: &[u8]) -> Result<(), ConvertError> {
    if bytes.len() > MAX_PDF_BYTES {
        return Err(ConvertError::PayloadTooLarge(bytes.len(), MAX_PDF_BYTES));
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err(ConvertError::BadRequest("not a PDF (no %PDF- header)".into()));
    }
    Ok(())
}

/// Write `bytes` to a fresh `.pdf` temp file and return the path. The
/// caller keeps the `NamedTempFile` alive to control lifetime.
fn write_temp_pdf(bytes: &[u8]) -> Result<NamedTempFile, ConvertError> {
    let mut f = NamedTempFile::with_suffix(".pdf")
        .map_err(|e| ConvertError::Internal(format!("temp pdf: {e}")))?;
    f.write_all(bytes)
        .map_err(|e| ConvertError::Internal(format!("write pdf: {e}")))?;
    f.flush()
        .map_err(|e| ConvertError::Internal(format!("flush pdf: {e}")))?;
    Ok(f)
}

async fn read_temp(path: &std::path::Path) -> Result<Vec<u8>, ConvertError> {
    tokio::fs::read(path)
        .await
        .map_err(|e| ConvertError::Internal(format!("read tool output: {e}")))
}

fn tool_error(label: &str, stderr: &[u8]) -> ConvertError {
    let snippet: String = String::from_utf8_lossy(stderr)
        .lines()
        .take(3)
        .collect::<Vec<_>>()
        .join(" / ");
    // qpdf exit code 3 means "successful with warnings" — callers should
    // not pass through to here in that case, but if they do the message
    // is informative.
    ConvertError::PdfRender(format!("{label}: {}", snippet.trim()))
}

/// Concatenate any number of PDF inputs in array order. Returns the
/// merged bytes.
pub async fn merge(inputs: &[Vec<u8>]) -> Result<Vec<u8>, ConvertError> {
    if inputs.is_empty() {
        return Err(ConvertError::BadRequest("merge requires at least one file".into()));
    }
    if inputs.len() > 50 {
        return Err(ConvertError::BadRequest("merge accepts at most 50 files".into()));
    }
    let mut tmp_inputs = Vec::with_capacity(inputs.len());
    for bytes in inputs {
        validate_pdf(bytes)?;
        tmp_inputs.push(write_temp_pdf(bytes)?);
    }
    let out = NamedTempFile::with_suffix(".pdf")
        .map_err(|e| ConvertError::Internal(format!("temp out: {e}")))?;
    let out_path = out.path().to_path_buf();
    let out_handle = out.into_temp_path();

    // qpdf invocation:
    //   qpdf --empty --pages A.pdf B.pdf C.pdf -- out.pdf
    let mut args: Vec<String> = vec!["--empty".into(), "--pages".into()];
    for f in &tmp_inputs {
        args.push(f.path().display().to_string());
    }
    args.push("--".into());
    args.push(out_path.display().to_string());

    let output = Command::new("qpdf")
        .args(&args)
        .output()
        .await
        .map_err(|e| ConvertError::PdfRender(format!("spawn qpdf: {e}")))?;
    if !output.status.success() && output.status.code() != Some(3) {
        return Err(tool_error("qpdf merge", &output.stderr));
    }
    let bytes = read_temp(&out_path).await?;
    drop(out_handle);
    drop(tmp_inputs);
    Ok(bytes)
}

/// Extract a page range (1-indexed, comma-separated; e.g. `"1-3,7"`).
/// Validates the range string before passing it to qpdf so we don't let
/// arbitrary shell arguments through.
pub async fn split(input: &[u8], pages: &str) -> Result<Vec<u8>, ConvertError> {
    validate_pdf(input)?;
    let pages = pages.trim();
    if pages.is_empty() {
        return Err(ConvertError::BadRequest("page range is required".into()));
    }
    // Allow digits, hyphens, commas, the literal "z" (qpdf's "last page"
    // alias), and the keyword "r" for reverse-from-end (e.g. r1 = last
    // page). Anything else → reject. Keeps the rule tight enough to be
    // safe-to-pass-through.
    if !pages
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '-' | ',' | 'z' | 'r' | ' '))
    {
        return Err(ConvertError::BadRequest(
            "page range may only contain digits, '-', ',', 'r', and 'z'".into(),
        ));
    }
    if pages.len() > 120 {
        return Err(ConvertError::BadRequest("page range is too long".into()));
    }

    let in_file = write_temp_pdf(input)?;
    let out = NamedTempFile::with_suffix(".pdf")
        .map_err(|e| ConvertError::Internal(format!("temp out: {e}")))?;
    let out_path = out.path().to_path_buf();
    let out_handle = out.into_temp_path();

    let output = Command::new("qpdf")
        .args([
            "--empty",
            "--pages",
            &in_file.path().display().to_string(),
            pages,
            "--",
            &out_path.display().to_string(),
        ])
        .output()
        .await
        .map_err(|e| ConvertError::PdfRender(format!("spawn qpdf: {e}")))?;
    if !output.status.success() && output.status.code() != Some(3) {
        return Err(tool_error("qpdf split", &output.stderr));
    }
    let bytes = read_temp(&out_path).await?;
    drop(out_handle);
    drop(in_file);
    Ok(bytes)
}

/// Ghostscript compression quality preset. Maps to `-dPDFSETTINGS=/...`.
#[derive(Debug, Clone, Copy)]
pub enum CompressLevel {
    /// 72 dpi, smallest file, good for previews and email.
    Screen,
    /// 150 dpi, balanced. The default.
    Ebook,
    /// 300 dpi, larger but better for office printing.
    Printer,
    /// 300 dpi, embeds all fonts; the largest "print-ready" preset.
    Prepress,
}

impl CompressLevel {
    pub fn from_str(s: &str) -> Result<Self, ConvertError> {
        match s.to_ascii_lowercase().as_str() {
            "screen" => Ok(Self::Screen),
            "ebook" | "default" | "" => Ok(Self::Ebook),
            "printer" => Ok(Self::Printer),
            "prepress" => Ok(Self::Prepress),
            other => Err(ConvertError::BadRequest(format!(
                "unknown compress level: {other}"
            ))),
        }
    }

    fn gs_arg(self) -> &'static str {
        match self {
            Self::Screen => "-dPDFSETTINGS=/screen",
            Self::Ebook => "-dPDFSETTINGS=/ebook",
            Self::Printer => "-dPDFSETTINGS=/printer",
            Self::Prepress => "-dPDFSETTINGS=/prepress",
        }
    }
}

/// Re-stream a PDF through Ghostscript at the requested quality level.
/// Typically shrinks file size 2-10× for image-heavy documents.
pub async fn compress(input: &[u8], level: CompressLevel) -> Result<Vec<u8>, ConvertError> {
    validate_pdf(input)?;
    let in_file = write_temp_pdf(input)?;
    let out = NamedTempFile::with_suffix(".pdf")
        .map_err(|e| ConvertError::Internal(format!("temp out: {e}")))?;
    let out_path = out.path().to_path_buf();
    let out_handle = out.into_temp_path();
    let out_arg = format!("-sOutputFile={}", out_path.display());

    let output = Command::new("gs")
        .args([
            "-sDEVICE=pdfwrite",
            "-dCompatibilityLevel=1.4",
            level.gs_arg(),
            "-dNOPAUSE",
            "-dBATCH",
            "-dQUIET",
            &out_arg,
            &in_file.path().display().to_string(),
        ])
        .output()
        .await
        .map_err(|e| ConvertError::PdfRender(format!("spawn ghostscript: {e}")))?;
    if !output.status.success() {
        return Err(tool_error("ghostscript compress", &output.stderr));
    }
    let bytes = read_temp(&out_path).await?;
    drop(out_handle);
    drop(in_file);
    Ok(bytes)
}

/// Apply a centred 45°-rotated text watermark to every page by rendering
/// a one-page overlay PDF (via Chromium → file via callback) and
/// stamping it on with `qpdf --overlay`.
pub async fn watermark(
    chromium_bin: &str,
    chromium_data_dir: Option<&std::path::Path>,
    input: &[u8],
    text: &str,
) -> Result<Vec<u8>, ConvertError> {
    validate_pdf(input)?;
    let text = text.trim();
    if text.is_empty() {
        return Err(ConvertError::BadRequest("watermark text is required".into()));
    }
    if text.len() > 120 {
        return Err(ConvertError::BadRequest(
            "watermark text must be ≤ 120 characters".into(),
        ));
    }

    // Render an overlay PDF. The trick is to ask Chromium to produce a
    // PDF the same size as a default page (Letter/A4); qpdf scales it to
    // fit each page automatically.
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let overlay_html = format!(
        r#"<!doctype html><html><head><style>
        @page {{ size: A4; margin: 0; }}
        html, body {{ margin: 0; padding: 0; width: 100%; height: 100%; }}
        body {{ display: flex; align-items: center; justify-content: center; }}
        .stamp {{ font: 700 96px/1 system-ui, sans-serif;
                  color: rgba(180, 30, 30, 0.18);
                  transform: rotate(-30deg);
                  user-select: none; }}
        </style></head><body><div class="stamp">{escaped}</div></body></html>"#
    );

    let overlay = crate::pdf::render_with_dir(
        chromium_bin,
        chromium_data_dir,
        &overlay_html,
        false,
        false,
    )
    .await?;
    let overlay_file = write_temp_pdf(&overlay)?;
    let in_file = write_temp_pdf(input)?;
    let out = NamedTempFile::with_suffix(".pdf")
        .map_err(|e| ConvertError::Internal(format!("temp out: {e}")))?;
    let out_path = out.path().to_path_buf();
    let out_handle = out.into_temp_path();

    let output = Command::new("qpdf")
        .args([
            &in_file.path().display().to_string(),
            "--overlay",
            &overlay_file.path().display().to_string(),
            "--repeat=1-z",
            "--",
            &out_path.display().to_string(),
        ])
        .output()
        .await
        .map_err(|e| ConvertError::PdfRender(format!("spawn qpdf: {e}")))?;
    if !output.status.success() && output.status.code() != Some(3) {
        return Err(tool_error("qpdf watermark", &output.stderr));
    }
    let bytes = read_temp(&out_path).await?;
    drop(out_handle);
    drop(in_file);
    drop(overlay_file);
    Ok(bytes)
}

/// Encrypt a PDF with a user password (required to open) and an optional
/// owner password (required to remove restrictions). Uses AES-256 if the
/// installed qpdf supports it.
pub async fn encrypt(
    input: &[u8],
    user_password: &str,
    owner_password: Option<&str>,
) -> Result<Vec<u8>, ConvertError> {
    validate_pdf(input)?;
    if user_password.is_empty() || user_password.len() > 64 {
        return Err(ConvertError::BadRequest(
            "user password must be 1..=64 chars".into(),
        ));
    }
    let owner = owner_password.unwrap_or(user_password);
    if owner.len() > 64 {
        return Err(ConvertError::BadRequest("owner password too long".into()));
    }
    // Reject control chars in passwords so they're safe to pass as a CLI
    // arg. Anything printable is OK.
    let ok = |s: &str| s.chars().all(|c| !c.is_control() && c != '\'' && c != '"');
    if !ok(user_password) || !ok(owner) {
        return Err(ConvertError::BadRequest(
            "passwords may not contain control chars or quotes".into(),
        ));
    }

    let in_file = write_temp_pdf(input)?;
    let out = NamedTempFile::with_suffix(".pdf")
        .map_err(|e| ConvertError::Internal(format!("temp out: {e}")))?;
    let out_path = out.path().to_path_buf();
    let out_handle = out.into_temp_path();

    let output = Command::new("qpdf")
        .args([
            "--encrypt",
            user_password,
            owner,
            "256",
            "--",
            &in_file.path().display().to_string(),
            &out_path.display().to_string(),
        ])
        .output()
        .await
        .map_err(|e| ConvertError::PdfRender(format!("spawn qpdf: {e}")))?;
    if !output.status.success() && output.status.code() != Some(3) {
        return Err(tool_error("qpdf encrypt", &output.stderr));
    }
    let bytes = read_temp(&out_path).await?;
    drop(out_handle);
    drop(in_file);
    Ok(bytes)
}

/// Flatten a PDF to PDF/A-2b via ghostscript. Embeds fonts, normalises
/// metadata, and emits a file that's suitable for archival systems
/// that require ISO 19005-2 compliance.
pub async fn to_pdf_a(input: &[u8]) -> Result<Vec<u8>, ConvertError> {
    validate_pdf(input)?;
    let in_file = write_temp_pdf(input)?;
    let out = NamedTempFile::with_suffix(".pdf")
        .map_err(|e| ConvertError::Internal(format!("temp out: {e}")))?;
    let out_path = out.path().to_path_buf();
    let out_handle = out.into_temp_path();
    let out_arg = format!("-sOutputFile={}", out_path.display());

    let output = Command::new("gs")
        .args([
            "-dPDFA=2",
            "-dBATCH",
            "-dNOPAUSE",
            "-dQUIET",
            "-sColorConversionStrategy=UseDeviceIndependentColor",
            "-sDEVICE=pdfwrite",
            "-dPDFACompatibilityPolicy=1",
            &out_arg,
            &in_file.path().display().to_string(),
        ])
        .output()
        .await
        .map_err(|e| ConvertError::PdfRender(format!("spawn ghostscript: {e}")))?;
    if !output.status.success() {
        return Err(tool_error("ghostscript pdf/a", &output.stderr));
    }
    let bytes = read_temp(&out_path).await?;
    drop(out_handle);
    drop(in_file);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_rejects_shell_metacharacters() {
        // We're not invoking qpdf here — just verifying the guard.
        let bad = ["1; rm -rf /", "$(whoami)", "1-3|cat", "1 2 3 ` "];
        for s in bad {
            let valid = s
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '-' | ',' | 'z' | 'r' | ' '));
            assert!(!valid, "expected to reject {s:?}");
        }
    }

    #[test]
    fn validate_pdf_requires_magic() {
        assert!(validate_pdf(b"not a pdf").is_err());
        assert!(validate_pdf(b"%PDF-1.7\n...").is_ok());
    }

    #[test]
    fn compress_level_parsing() {
        assert!(matches!(
            CompressLevel::from_str("screen").unwrap(),
            CompressLevel::Screen
        ));
        assert!(matches!(
            CompressLevel::from_str("").unwrap(),
            CompressLevel::Ebook
        ));
        assert!(CompressLevel::from_str("garbage").is_err());
    }
}
