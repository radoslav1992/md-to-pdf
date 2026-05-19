use serde::{Deserialize, Serialize};

use crate::db::{AppState, User};
use crate::error::ConvertError;
use crate::pandoc;
use crate::pdf;
use crate::themes;

pub const MAX_INPUT_BYTES_FREE: usize = 256 * 1024; // 256 KiB for anonymous + free
pub const MAX_INPUT_BYTES_PREMIUM: usize = 16 * 1024 * 1024; // 16 MiB for premium/admin

/// Inputs available without a premium subscription.
const FREE_INPUTS: &[&str] = &["markdown", "md"];

/// All input types the backend knows how to render.
const ALL_INPUTS: &[&str] = &[
    "markdown", "md", "html", "json", "xml", "csv", "org", "asciidoc", "adoc", "rst", "latex",
    "tex",
];

#[derive(Debug, Deserialize)]
pub struct ConvertFile {
    /// Filename within the bundle. Currently informational only — files are
    /// concatenated in the order they appear in the array.
    #[allow(dead_code)]
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct PdfMargin {
    pub top: Option<String>,
    pub right: Option<String>,
    pub bottom: Option<String>,
    pub left: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct PdfCover {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
}

/// Page-setup controls for the PDF output. All fields are optional.
#[derive(Debug, Deserialize, Default)]
pub struct PdfOptions {
    pub page_size: Option<String>,
    pub orientation: Option<String>,
    pub margin: Option<PdfMargin>,
    pub page_numbers: Option<bool>,
    pub header_template: Option<String>,
    pub footer_template: Option<String>,
    pub cover: Option<PdfCover>,
}

#[derive(Debug, Deserialize)]
pub struct ConvertRequest {
    #[serde(rename = "type")]
    pub input_type: String,
    #[serde(default = "default_output")]
    pub output: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub files: Option<Vec<ConvertFile>>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub custom_css: Option<String>,
    #[serde(default)]
    pub pdf_options: Option<PdfOptions>,
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

    if !ALL_INPUTS.contains(&normalized_input.as_str()) {
        return Err(ConvertError::UnsupportedInput(req.input_type.clone()));
    }
    if !is_premium && !FREE_INPUTS.contains(&normalized_input.as_str()) {
        return Err(ConvertError::PremiumRequired);
    }

    // Premium gating for the new fields. We don't silently downgrade — fail
    // loudly so free users get a clear upgrade nudge instead of mysterious
    // output.
    let theme_name = req.theme.as_deref().unwrap_or("default");
    if !is_premium && themes::is_premium_theme(theme_name) {
        return Err(ConvertError::PremiumRequired);
    }
    if !is_premium && req.custom_css.as_deref().is_some_and(|s| !s.is_empty()) {
        return Err(ConvertError::PremiumRequired);
    }
    if !is_premium && req.pdf_options.is_some() {
        return Err(ConvertError::PremiumRequired);
    }
    if !is_premium && req.files.as_ref().is_some_and(|f| !f.is_empty()) {
        return Err(ConvertError::PremiumRequired);
    }

    // Resolve input bytes: multi-file mode concatenates files in array order
    // with a blank line between them. Markdown is the only input where multi-
    // file is genuinely useful, but we accept it for any text format.
    let combined: String;
    let raw_input = match req.files.as_ref() {
        Some(files) if !files.is_empty() => {
            if !matches!(
                normalized_input.as_str(),
                "markdown" | "md" | "html" | "asciidoc" | "adoc" | "rst" | "latex" | "tex" | "org"
            ) {
                return Err(ConvertError::BadRequest(
                    "multi-file input is only supported for text formats".into(),
                ));
            }
            combined = join_files(files);
            combined.as_str()
        }
        _ => req.content.as_str(),
    };

    let max = if is_premium {
        MAX_INPUT_BYTES_PREMIUM
    } else {
        MAX_INPUT_BYTES_FREE
    };
    if raw_input.len() > max {
        return Err(ConvertError::PayloadTooLarge(raw_input.len(), max));
    }

    let body_html = match normalized_input.as_str() {
        "markdown" | "md" => markdown_to_html(raw_input),
        "html" => sanitize_html(raw_input),
        "json" => json_to_html(raw_input)?,
        "xml" => xml_to_html(raw_input)?,
        "csv" => csv_to_html(raw_input)?,
        "org" => org_to_html(raw_input)?,
        "asciidoc" | "adoc" => sanitize_html(&pandoc::to_html("asciidoc", raw_input).await?),
        "rst" => sanitize_html(&pandoc::to_html("rst", raw_input).await?),
        "latex" | "tex" => sanitize_html(&pandoc::to_html("latex", raw_input).await?),
        other => return Err(ConvertError::UnsupportedInput(other.to_string())),
    };

    let title = req.title.clone().unwrap_or_else(|| "Document".to_string());
    let document = wrap_document(
        &title,
        &body_html,
        theme_name,
        req.custom_css.as_deref(),
        req.pdf_options.as_ref(),
    );

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
            let _permit = state.pdf_semaphore.acquire().await.map_err(|e| {
                ConvertError::Internal(format!("failed to acquire pdf permit: {e}"))
            })?;
            let want_numbers = req
                .pdf_options
                .as_ref()
                .and_then(|o| o.page_numbers)
                .unwrap_or(false);
            let pdf_bytes = pdf::render(&state.chromium_bin, &document, want_numbers).await?;
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

fn join_files(files: &[ConvertFile]) -> String {
    let mut out = String::new();
    for f in files {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&f.content);
    }
    out
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
        .add_tags(["details", "summary", "section", "article", "header", "footer"])
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

fn csv_to_html(input: &str) -> Result<String, ConvertError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(input.as_bytes());

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| ConvertError::Parse {
            format: "csv",
            message: e.to_string(),
        })?
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut html = String::from("<table>\n  <thead>\n    <tr>");
    for h in &headers {
        html.push_str(&format!("<th>{}</th>", html_escape(h)));
    }
    html.push_str("</tr>\n  </thead>\n  <tbody>\n");

    for record in reader.records() {
        let record = record.map_err(|e| ConvertError::Parse {
            format: "csv",
            message: e.to_string(),
        })?;
        html.push_str("    <tr>");
        for field in record.iter() {
            html.push_str(&format!("<td>{}</td>", html_escape(field)));
        }
        html.push_str("</tr>\n");
    }
    html.push_str("  </tbody>\n</table>");
    Ok(html)
}

fn org_to_html(input: &str) -> Result<String, ConvertError> {
    let parsed = orgize::Org::parse(input);
    let mut out = Vec::new();
    parsed
        .write_html(&mut out)
        .map_err(|e| ConvertError::Parse {
            format: "org",
            message: e.to_string(),
        })?;
    let html = String::from_utf8_lossy(&out).into_owned();
    Ok(sanitize_html(&html))
}

fn wrap_document(
    title: &str,
    body: &str,
    theme: &str,
    custom_css: Option<&str>,
    pdf_options: Option<&PdfOptions>,
) -> String {
    let theme_css = themes::css_for(theme);
    let page_css = pdf_options.map(page_css).unwrap_or_default();
    let custom = custom_css.unwrap_or("");
    let cover = pdf_options
        .and_then(|o| o.cover.as_ref())
        .map(cover_html)
        .unwrap_or_default();
    let header = pdf_options
        .and_then(|o| o.header_template.as_deref())
        .filter(|s| !s.is_empty())
        .map(|h| {
            format!(
                "<div class=\"_page_header\">{}</div>",
                sanitize_html(h)
            )
        })
        .unwrap_or_default();
    let footer = pdf_options
        .and_then(|o| o.footer_template.as_deref())
        .filter(|s| !s.is_empty())
        .map(|f| {
            format!(
                "<div class=\"_page_footer\">{}</div>",
                sanitize_html(f)
            )
        })
        .unwrap_or_default();

    let header_footer_css = if !header.is_empty() || !footer.is_empty() {
        r#"
  ._page_header { position: fixed; top: 0; left: 0; right: 0; text-align: center; font-size: 0.8em; color: #6b7280; padding: 0.3em 0; }
  ._page_footer { position: fixed; bottom: 0; left: 0; right: 0; text-align: center; font-size: 0.8em; color: #6b7280; padding: 0.3em 0; }
"#
    } else {
        ""
    };

    let cover_css = if !cover.is_empty() {
        r#"
  ._cover { page-break-after: always; min-height: 90vh; display: flex; flex-direction: column; justify-content: center; align-items: center; text-align: center; }
  ._cover h1 { font-size: 2.6em; margin: 0 0 0.2em; border: none; }
  ._cover ._subtitle { font-size: 1.3em; color: #475569; margin-bottom: 2em; }
  ._cover ._author { font-size: 1.05em; color: #1f2937; }
  ._cover ._date { font-size: 0.95em; color: #6b7280; margin-top: 0.4em; }
"#
    } else {
        ""
    };

    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"/>
<title>{title}</title>
<style>{theme_css}{header_footer_css}{cover_css}{page_css}{custom}</style>
</head><body>{header}{cover}{body}{footer}</body></html>"#,
        title = html_escape(title),
        theme_css = theme_css,
        header_footer_css = header_footer_css,
        cover_css = cover_css,
        page_css = page_css,
        custom = custom,
        header = header,
        cover = cover,
        body = body,
        footer = footer,
    )
}

fn page_css(options: &PdfOptions) -> String {
    let size = options
        .page_size
        .as_deref()
        .map(normalize_page_size)
        .unwrap_or_else(|| "A4".to_string());
    let orientation = match options.orientation.as_deref() {
        Some("landscape") => " landscape",
        Some("portrait") => " portrait",
        _ => "",
    };

    let margin = options
        .margin
        .as_ref()
        .map(format_margin)
        .unwrap_or_else(|| "1in".to_string());

    format!(
        "\n  @page {{ size: {size}{orientation}; margin: {margin}; }}\n"
    )
}

fn normalize_page_size(input: &str) -> String {
    match input.to_ascii_lowercase().as_str() {
        "a3" => "A3".to_string(),
        "a4" => "A4".to_string(),
        "a5" => "A5".to_string(),
        "letter" => "Letter".to_string(),
        "legal" => "Legal".to_string(),
        "tabloid" => "Tabloid".to_string(),
        other => {
            // Accept raw CSS values like "8.5in 11in"; pass through after a
            // very minimal sanity check.
            if other
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ' ' | 'i' | 'n' | 'c' | 'm'))
            {
                input.to_string()
            } else {
                "A4".to_string()
            }
        }
    }
}

fn format_margin(margin: &PdfMargin) -> String {
    let top = margin.top.as_deref().unwrap_or("1in");
    let right = margin.right.as_deref().unwrap_or("1in");
    let bottom = margin.bottom.as_deref().unwrap_or("1in");
    let left = margin.left.as_deref().unwrap_or("1in");
    // Strict CSS length validation: alphanumerics, dot, percent only.
    let safe = |s: &str| {
        s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '%'))
            && !s.is_empty()
            && s.len() <= 12
    };
    if safe(top) && safe(right) && safe(bottom) && safe(left) {
        format!("{top} {right} {bottom} {left}")
    } else {
        "1in".to_string()
    }
}

fn cover_html(cover: &PdfCover) -> String {
    let mut out = String::from("<section class=\"_cover\">");
    if let Some(t) = cover.title.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("<h1>{}</h1>", html_escape(t)));
    }
    if let Some(s) = cover.subtitle.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!(
            "<p class=\"_subtitle\">{}</p>",
            html_escape(s)
        ));
    }
    if let Some(a) = cover.author.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("<p class=\"_author\">{}</p>", html_escape(a)));
    }
    if let Some(d) = cover.date.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("<p class=\"_date\">{}</p>", html_escape(d)));
    }
    out.push_str("</section>");
    out
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

    #[test]
    fn csv_simple_renders_table() {
        let out = csv_to_html("name,age\nAlice,30\nBob,25").unwrap();
        assert!(out.contains("<table>"));
        assert!(out.contains("<th>name</th>"));
        assert!(out.contains("<td>Alice</td>"));
        assert!(out.contains("<td>25</td>"));
    }

    #[test]
    fn csv_escapes_html_in_cells() {
        let out = csv_to_html("col\n<script>x</script>").unwrap();
        assert!(!out.contains("<script>x</script>"));
        assert!(out.contains("&lt;script&gt;"));
    }

    #[test]
    fn org_renders_to_html() {
        let out = org_to_html("* Hello\n\nbody").unwrap();
        assert!(out.to_lowercase().contains("hello"));
    }

    #[test]
    fn page_css_defaults_to_a4() {
        let css = page_css(&PdfOptions::default());
        assert!(css.contains("size: A4"));
        assert!(css.contains("margin: 1in"));
    }

    #[test]
    fn page_css_orientation_and_margin() {
        let opts = PdfOptions {
            page_size: Some("Letter".into()),
            orientation: Some("landscape".into()),
            margin: Some(PdfMargin {
                top: Some("0.5in".into()),
                right: Some("0.75in".into()),
                bottom: Some("0.5in".into()),
                left: Some("0.75in".into()),
            }),
            ..Default::default()
        };
        let css = page_css(&opts);
        assert!(css.contains("size: Letter landscape"));
        assert!(css.contains("0.5in 0.75in 0.5in 0.75in"));
    }

    #[test]
    fn page_css_rejects_injection_in_margin() {
        let opts = PdfOptions {
            margin: Some(PdfMargin {
                top: Some("1in; } body { display: none".into()),
                right: None,
                bottom: None,
                left: None,
            }),
            ..Default::default()
        };
        let css = page_css(&opts);
        assert!(css.contains("margin: 1in"));
        assert!(!css.contains("display: none"));
    }

    #[test]
    fn cover_html_escapes_text() {
        let cover = PdfCover {
            title: Some("<x>".into()),
            ..Default::default()
        };
        let out = cover_html(&cover);
        assert!(out.contains("&lt;x&gt;"));
    }

    #[test]
    fn wrap_document_injects_theme_css() {
        let doc = wrap_document("t", "<p>x</p>", "github", None, None);
        assert!(doc.contains("border-bottom: 1px solid #d1d9e0"));
    }

    #[test]
    fn join_files_concatenates_in_order() {
        let files = vec![
            ConvertFile {
                path: "a.md".into(),
                content: "# A".into(),
            },
            ConvertFile {
                path: "b.md".into(),
                content: "# B".into(),
            },
        ];
        let out = join_files(&files);
        assert_eq!(out, "# A\n\n# B");
    }
}
