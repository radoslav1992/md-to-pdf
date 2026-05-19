//! Pandoc-based conversion for input formats with no Rust-native parser:
//! AsciiDoc, reStructuredText, and LaTeX. Pandoc is spawned as a subprocess
//! and reads input from stdin to avoid temp files.
//!
//! Also drives the inverse path — wrapping our generated HTML and shelling
//! out to pandoc to produce DOCX / EPUB / ODT / GFM-Markdown. Those binary
//! outputs are read off pandoc's stdout into a `Vec<u8>` and returned to
//! the caller, who base64-encodes them for the JSON response.
//!
//! If the `pandoc` binary is not available the call returns a parse error;
//! `pandoc_available()` lets callers skip those tests in environments
//! without pandoc installed.

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::ConvertError;

/// Pandoc input format -> HTML output. Returns parse error on bad input or
/// when pandoc is not installed in the container.
pub async fn to_html(format: &str, input: &str) -> Result<String, ConvertError> {
    let mut child = Command::new("pandoc")
        .args([
            "-f",
            format,
            "-t",
            "html5",
            "--no-highlight",
            "--wrap=preserve",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| ConvertError::Parse {
            format: pandoc_format_label(format),
            message: format!("pandoc not available: {e}"),
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .await
            .map_err(|e| ConvertError::Parse {
                format: pandoc_format_label(format),
                message: format!("write to pandoc: {e}"),
            })?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| ConvertError::Parse {
            format: pandoc_format_label(format),
            message: format!("wait pandoc: {e}"),
        })?;

    if !output.status.success() {
        return Err(ConvertError::Parse {
            format: pandoc_format_label(format),
            message: String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("pandoc failed")
                .to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// HTML → some pandoc-supported output (docx / epub / odt). Returns the
/// raw bytes so the caller can base64-encode them in the JSON response.
pub async fn from_html_to_bytes(target: &str, html: &str) -> Result<Vec<u8>, ConvertError> {
    // `--standalone` is required for binary outputs so they're well-formed
    // archives. `--embed-resources` would also bake in `<img src="data:…">`
    // automatically, but our pipeline already inlines images for PDF and
    // pandoc happily picks up data URIs from the HTML — so we keep the
    // command minimal.
    let mut child = Command::new("pandoc")
        .args(["-f", "html", "-t", target, "--standalone"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| ConvertError::Internal(format!("pandoc not available: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(html.as_bytes())
            .await
            .map_err(|e| ConvertError::Internal(format!("write to pandoc: {e}")))?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| ConvertError::Internal(format!("wait pandoc: {e}")))?;

    if !output.status.success() {
        return Err(ConvertError::Internal(format!(
            "pandoc -> {target} failed: {}",
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .next()
                .unwrap_or("(no stderr)")
        )));
    }
    Ok(output.stdout)
}

/// HTML → a text output (gfm Markdown, plain). Same wiring as
/// `from_html_to_bytes` but UTF-8 decoded.
pub async fn from_html_to_text(target: &str, html: &str) -> Result<String, ConvertError> {
    let bytes = from_html_to_bytes(target, html).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn pandoc_format_label(format: &str) -> &'static str {
    match format {
        "asciidoc" => "asciidoc",
        "rst" => "rst",
        "latex" => "latex",
        _ => "pandoc",
    }
}
