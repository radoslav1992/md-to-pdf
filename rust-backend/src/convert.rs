use serde::{Deserialize, Serialize};

use crate::db::{AppState, User};
use crate::error::ConvertError;
use crate::pdf;

const MAX_INPUT_BYTES_FREE: usize = 256 * 1024; // 256 KiB for anonymous + free
const MAX_INPUT_BYTES_PREMIUM: usize = 4 * 1024 * 1024; // 4 MiB for premium/admin

const FREE_INPUTS: &[&str] = &["markdown", "md"];

#[derive(Debug, Deserialize)]
pub struct ConvertRequest {
    #[serde(rename = "type")]
    pub input_type: String,
    #[serde(default = "default_output")]
    pub output: String,
    pub content: String,
    #[serde(default)]
    pub title: Option<String>,
}

fn default_output() -> String {
    "html".to_string()
}

#[derive(Debug, Serialize)]
pub struct ConvertResponse {
    pub ok: bool,
    pub output_type: String,
    pub input_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf_base64: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

pub async fn run(
    req: &ConvertRequest,
    state: &AppState,
    user: Option<&User>,
) -> Result<ConvertResponse, ConvertError> {
    let normalized_input = req.input_type.to_ascii_lowercase();
    let is_premium = user.map(|u| u.is_premium()).unwrap_or(false);

    if !is_premium && !FREE_INPUTS.contains(&normalized_input.as_str()) {
        return Err(ConvertError::PremiumRequired);
    }

    let max = if is_premium {
        MAX_INPUT_BYTES_PREMIUM
    } else {
        MAX_INPUT_BYTES_FREE
    };
    if req.content.len() > max {
        return Err(ConvertError::PayloadTooLarge(req.content.len(), max));
    }

    let html = match normalized_input.as_str() {
        "markdown" | "md" => markdown_to_html(&req.content),
        "html" => sanitize_html(&req.content),
        "json" => json_to_html(&req.content)?,
        "xml" => xml_to_html(&req.content)?,
        other => return Err(ConvertError::UnsupportedInput(other.to_string())),
    };

    let title = req.title.clone().unwrap_or_else(|| "Document".to_string());
    let document = wrap_document(&title, &html);

    match req.output.to_ascii_lowercase().as_str() {
        "html" => Ok(ConvertResponse {
            ok: true,
            output_type: "html".to_string(),
            input_type: normalized_input,
            content: Some(document),
            pdf_base64: None,
            warnings: Vec::new(),
        }),
        "pdf" => {
            let pdf_bytes = pdf::render(&state.chromium_bin, &document).await?;
            let encoded = pdf::encode_base64(&pdf_bytes);
            Ok(ConvertResponse {
                ok: true,
                output_type: "pdf".to_string(),
                input_type: normalized_input,
                content: None,
                pdf_base64: Some(encoded),
                warnings: Vec::new(),
            })
        }
        other => Err(ConvertError::UnsupportedOutput(other.to_string())),
    }
}

fn markdown_to_html(input: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let parser = Parser::new_ext(input, options);
    let mut html_out = String::with_capacity(input.len() + 256);
    html::push_html(&mut html_out, parser);
    sanitize_html(&html_out)
}

fn sanitize_html(input: &str) -> String {
    ammonia::Builder::default()
        .add_tags(["details", "summary"])
        .add_generic_attributes(["class", "id"])
        .clean(input)
        .to_string()
}

fn json_to_html(input: &str) -> Result<String, ConvertError> {
    let value: serde_json::Value =
        serde_json::from_str(input).map_err(|e| ConvertError::Parse {
            format: "json",
            message: e.to_string(),
        })?;
    let pretty = serde_json::to_string_pretty(&value).unwrap_or_else(|_| input.to_string());
    Ok(format!(
        "<pre class=\"language-json\"><code>{}</code></pre>",
        html_escape(&pretty)
    ))
}

fn xml_to_html(input: &str) -> Result<String, ConvertError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(ConvertError::Parse {
                    format: "xml",
                    message: e.to_string(),
                });
            }
        }
        buf.clear();
    }
    Ok(format!(
        "<pre class=\"language-xml\"><code>{}</code></pre>",
        html_escape(input)
    ))
}

fn wrap_document(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"/>
<title>{title}</title>
<style>
  body {{ font-family: -apple-system, system-ui, sans-serif; max-width: 760px; margin: 2rem auto; padding: 0 1rem; line-height: 1.55; color: #0f172a; }}
  pre {{ background: #0f172a; color: #e2e8f0; padding: 1rem; border-radius: 8px; overflow-x: auto; }}
  code {{ font-family: ui-monospace, monospace; font-size: 0.92em; }}
  h1, h2, h3 {{ color: #1e3a8a; }}
  a {{ color: #2563eb; }}
  table {{ border-collapse: collapse; width: 100%; }}
  th, td {{ border: 1px solid #cbd5e1; padding: 6px 10px; text-align: left; }}
</style>
</head><body>{body}</body></html>"#,
        title = html_escape(title),
        body = body,
    )
}

fn html_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_renders_headings() {
        let out = markdown_to_html("# Hello\n\nbody");
        assert!(out.contains("<h1>Hello</h1>"));
        assert!(out.contains("<p>body</p>"));
    }

    #[test]
    fn markdown_strips_script_tags() {
        let out = markdown_to_html("<script>alert(1)</script>\n\ntext");
        assert!(!out.contains("<script"));
        assert!(out.contains("text"));
    }

    #[test]
    fn json_invalid_is_rejected() {
        let err = json_to_html("{not json").unwrap_err();
        match err {
            ConvertError::Parse { format, .. } => assert_eq!(format, "json"),
            _ => panic!("expected parse error"),
        }
    }

    #[test]
    fn xml_invalid_is_rejected() {
        let err = xml_to_html("<a><b></a>").unwrap_err();
        match err {
            ConvertError::Parse { format, .. } => assert_eq!(format, "xml"),
            _ => panic!("expected parse error"),
        }
    }

    #[test]
    fn json_valid_produces_pre() {
        let out = json_to_html(r#"{"a":1}"#).unwrap();
        assert!(out.starts_with("<pre"));
    }
}
