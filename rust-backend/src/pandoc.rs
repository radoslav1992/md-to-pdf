//! Pandoc-based conversion for input formats with no Rust-native parser:
//! AsciiDoc, reStructuredText, and LaTeX. Pandoc is spawned as a subprocess
//! and reads input from stdin to avoid temp files.
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

fn pandoc_format_label(format: &str) -> &'static str {
    match format {
        "asciidoc" => "asciidoc",
        "rst" => "rst",
        "latex" => "latex",
        _ => "pandoc",
    }
}
