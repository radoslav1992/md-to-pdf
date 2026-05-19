use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::db::{AppState, User};
use crate::enrichments::Enrichments;
use crate::error::ConvertError;
use crate::images;
use crate::pandoc;
use crate::pdf;
use crate::render_cache::{self, CachedRender};
use crate::templates;
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

#[derive(Debug, Deserialize, Serialize)]
pub struct ConvertFile {
    /// Filename within the bundle. Currently informational only — files are
    /// concatenated in the order they appear in the array.
    #[allow(dead_code)]
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct PdfMargin {
    pub top: Option<String>,
    pub right: Option<String>,
    pub bottom: Option<String>,
    pub left: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct PdfCover {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
}

/// Page-setup controls for the PDF output. All fields are optional.
#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct PdfOptions {
    pub page_size: Option<String>,
    pub orientation: Option<String>,
    pub margin: Option<PdfMargin>,
    pub page_numbers: Option<bool>,
    pub header_template: Option<String>,
    pub footer_template: Option<String>,
    pub cover: Option<PdfCover>,
    /// When `true`, post-process the rendered PDF through ghostscript
    /// to produce a PDF/A-2b archival-grade file. Adds ~1s per render.
    pub pdf_a: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct EnrichmentOptions {
    #[serde(default)]
    pub toc: bool,
    #[serde(default)]
    pub toc_depth: Option<u8>,
    #[serde(default)]
    pub syntax_highlight: bool,
    #[serde(default)]
    pub math: bool,
    #[serde(default)]
    pub mermaid: bool,
}

impl EnrichmentOptions {
    pub fn is_active(&self) -> bool {
        self.toc || self.syntax_highlight || self.math || self.mermaid
    }
}

#[derive(Debug, Deserialize, Serialize)]
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
    /// When set, the named template is loaded and its theme / custom_css /
    /// pdf_options are used as defaults — request-level fields still win
    /// if explicitly provided.
    #[serde(default)]
    pub template_id: Option<i64>,
    #[serde(default)]
    pub enrichments: Option<EnrichmentOptions>,
}

fn default_output() -> String {
    "html".to_string()
}

#[derive(Debug, Serialize)]
pub struct ConvertResponse {
    pub ok: bool,
    pub output_type: String,
    pub input_type: String,
    /// Populated for text outputs (HTML, Markdown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Backward-compat alias; only set when `output == "pdf"`. New clients
    /// should prefer `output_base64` + `output_mime`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf_base64: Option<String>,
    /// Base64 bytes for any binary output (pdf, docx, epub, odt, png, jpg).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_base64: Option<String>,
    /// Content-Type for the bytes in `output_base64`. Lets the editor pick
    /// the right download filename / preview UI without hard-coding cases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_mime: Option<String>,
    /// `true` when the response came from the render cache. Diagnostic
    /// only — clients can ignore it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cached: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Output formats that produce binary bytes (not text). Anything not in
/// this list — `html`, `markdown` — flows through `ConvertResponse::content`.
const BINARY_OUTPUTS: &[&str] = &["pdf", "docx", "epub", "odt", "png", "jpg", "jpeg"];

/// Outputs we cache. HTML is excluded because re-running the pipeline is
/// already cheap (no subprocess) and clients hitting it on every keystroke
/// (live preview) would otherwise pollute the cache.
const CACHEABLE_OUTPUTS: &[&str] =
    &["pdf", "docx", "epub", "odt", "png", "jpg", "jpeg", "markdown", "md"];

fn output_mime(output: &str) -> &'static str {
    match output {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "epub" => "application/epub+zip",
        "odt" => "application/vnd.oasis.opendocument.text",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "html" => "text/html; charset=utf-8",
        "markdown" | "md" => "text/markdown; charset=utf-8",
        _ => "application/octet-stream",
    }
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

    // Templates expand into request-level fields. Request-level explicit
    // values still win — the template only provides defaults.
    let (template_theme, template_css, template_pdf_options) =
        if let Some(template_id) = req.template_id {
            if !is_premium {
                return Err(ConvertError::PremiumRequired);
            }
            let user = user.expect("premium implies authed");
            let tmpl = templates::get(&state.pool, user, template_id).await?;
            let pdf_opts = tmpl
                .pdf_options
                .as_deref()
                .and_then(|s| serde_json::from_str::<PdfOptions>(s).ok());
            (tmpl.theme, tmpl.custom_css, pdf_opts)
        } else {
            (None, None, None)
        };

    let theme_name = req
        .theme
        .clone()
        .or(template_theme)
        .unwrap_or_else(|| "default".to_string());
    let custom_css = req.custom_css.clone().or(template_css);
    let pdf_options = req.pdf_options.clone().or(template_pdf_options);

    // Premium gating for the new fields. We don't silently downgrade — fail
    // loudly so free users get a clear upgrade nudge instead of mysterious
    // output.
    if !is_premium && themes::is_premium_theme(&theme_name) {
        return Err(ConvertError::PremiumRequired);
    }
    if !is_premium && custom_css.as_deref().is_some_and(|s| !s.is_empty()) {
        return Err(ConvertError::PremiumRequired);
    }
    if !is_premium && pdf_options.is_some() {
        return Err(ConvertError::PremiumRequired);
    }
    if !is_premium && req.files.as_ref().is_some_and(|f| !f.is_empty()) {
        return Err(ConvertError::PremiumRequired);
    }
    if !is_premium && req.enrichments.as_ref().is_some_and(|e| e.is_active()) {
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

    let mut body_html = match normalized_input.as_str() {
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

    let mut has_mermaid = false;
    if let Some(opts) = req.enrichments.as_ref().filter(|o| o.is_active()) {
        let e = Enrichments {
            toc: opts.toc,
            toc_depth: opts.toc_depth.unwrap_or(3),
            syntax_highlight: opts.syntax_highlight,
            math: opts.math,
            mermaid: opts.mermaid,
        };
        body_html = crate::enrichments::apply(&body_html, &e);
        has_mermaid = e.mermaid && crate::enrichments::document_has_mermaid(&body_html);
    }

    let title = req.title.clone().unwrap_or_else(|| "Document".to_string());
    let document = wrap_document(
        &title,
        &body_html,
        &theme_name,
        custom_css.as_deref(),
        pdf_options.as_ref(),
        has_mermaid,
    );

    let output = req.output.to_ascii_lowercase();
    if output != "html"
        && output != "pdf"
        && output != "markdown"
        && output != "md"
        && !BINARY_OUTPUTS.contains(&output.as_str())
    {
        return Err(ConvertError::UnsupportedOutput(output));
    }

    // ---- cache lookup ----
    let pdf_options_json = pdf_options
        .as_ref()
        .and_then(|o| serde_json::to_string(o).ok());
    let enrichments_json = req
        .enrichments
        .as_ref()
        .filter(|e| e.is_active())
        .and_then(|e| serde_json::to_string(e).ok());
    let cache_key = render_cache::key(
        user.map(|u| u.id),
        &normalized_input,
        &output,
        &req.content,
        &theme_name,
        custom_css.as_deref(),
        pdf_options_json.as_deref(),
        enrichments_json.as_deref(),
        &title,
        req.template_id,
    );
    if CACHEABLE_OUTPUTS.contains(&output.as_str()) {
        let hit = {
            let mut cache = state.render_cache.lock().expect("render cache poisoned");
            cache.get(&cache_key)
        };
        if let Some(hit) = hit {
            return Ok(response_from_cached(&output, &hit, true));
        }
    }

    // ---- actual render ----
    let response = match output.as_str() {
        "html" => ConvertResponse {
            ok: true,
            output_type: "html".to_string(),
            input_type: normalized_input.clone(),
            content: Some(document.clone()),
            pdf_base64: None,
            output_base64: None,
            output_mime: Some(output_mime("html").to_string()),
            cached: false,
            warnings: Vec::new(),
        },
        "pdf" => {
            let slot = state.chromium_pool.acquire().await?;
            let want_numbers = pdf_options
                .as_ref()
                .and_then(|o| o.page_numbers)
                .unwrap_or(false);
            // Inline any `/api/images/N` references as data: URIs so Chromium
            // (loading the page over `file://`) can render them. Skipped for
            // anonymous users because they can't own images.
            let doc = if let Some(u) = user {
                inline_user_images(&document, &state.pool, u).await
            } else {
                document.clone()
            };
            let mut pdf_bytes = pdf::render_with_dir(
                &state.chromium_bin,
                Some(slot.data_dir()),
                &doc,
                want_numbers,
                has_mermaid,
            )
            .await?;
            // Optional PDF/A flattening. Off by default — adds a
            // ghostscript pass — but turning it on is one extra option.
            let want_pdf_a = pdf_options.as_ref().and_then(|o| o.pdf_a).unwrap_or(false);
            if want_pdf_a {
                pdf_bytes = crate::pdf_tools::to_pdf_a(&pdf_bytes).await?;
            }
            let encoded = pdf::encode_base64(&pdf_bytes);
            ConvertResponse {
                ok: true,
                output_type: "pdf".to_string(),
                input_type: normalized_input.clone(),
                content: None,
                pdf_base64: Some(encoded.clone()),
                output_base64: Some(encoded),
                output_mime: Some(output_mime("pdf").to_string()),
                cached: false,
                warnings: Vec::new(),
            }
        }
        "png" | "jpg" | "jpeg" => {
            let slot = state.chromium_pool.acquire().await?;
            let doc = if let Some(u) = user {
                inline_user_images(&document, &state.pool, u).await
            } else {
                document.clone()
            };
            let format = match output.as_str() {
                "png" => pdf::ImageFormat::Png,
                _ => pdf::ImageFormat::Jpeg,
            };
            let bytes = pdf::screenshot_with_dir(
                &state.chromium_bin,
                Some(slot.data_dir()),
                &doc,
                format,
                has_mermaid,
            )
            .await?;
            ConvertResponse {
                ok: true,
                output_type: output.clone(),
                input_type: normalized_input.clone(),
                content: None,
                pdf_base64: None,
                output_base64: Some(pdf::encode_base64(&bytes)),
                output_mime: Some(output_mime(&output).to_string()),
                cached: false,
                warnings: Vec::new(),
            }
        }
        "docx" | "epub" | "odt" => {
            // Inline images first so pandoc can embed them. Pandoc reads
            // `<img src="data:...">` fine and bakes the bytes into the
            // resulting archive.
            let doc = if let Some(u) = user {
                inline_user_images(&document, &state.pool, u).await
            } else {
                document.clone()
            };
            let bytes = pandoc::from_html_to_bytes(&output, &doc).await?;
            ConvertResponse {
                ok: true,
                output_type: output.clone(),
                input_type: normalized_input.clone(),
                content: None,
                pdf_base64: None,
                output_base64: Some(pdf::encode_base64(&bytes)),
                output_mime: Some(output_mime(&output).to_string()),
                cached: false,
                warnings: Vec::new(),
            }
        }
        "markdown" | "md" => {
            // We always go HTML → pandoc → gfm-markdown. Lossy compared to
            // round-tripping the original Markdown verbatim, but the upside
            // is that enrichments (TOC, syntax-highlighted code, math) are
            // already baked in, and every input type funnels through the
            // same code path.
            let md = pandoc::from_html_to_text("gfm", &document).await?;
            ConvertResponse {
                ok: true,
                output_type: "markdown".to_string(),
                input_type: normalized_input.clone(),
                content: Some(md),
                pdf_base64: None,
                output_base64: None,
                output_mime: Some(output_mime("markdown").to_string()),
                cached: false,
                warnings: Vec::new(),
            }
        }
        other => return Err(ConvertError::UnsupportedOutput(other.to_string())),
    };

    // ---- cache insert ----
    if CACHEABLE_OUTPUTS.contains(&output.as_str()) {
        if let Some(cached) = response_to_cache_entry(&response) {
            let mut cache = state.render_cache.lock().expect("render cache poisoned");
            cache.insert(cache_key, cached);
        }
    }

    Ok(response)
}

/// Translate a fresh `ConvertResponse` into the shape we store in the
/// cache. Returns `None` for outputs that aren't worth caching (e.g. the
/// content is empty).
fn response_to_cache_entry(resp: &ConvertResponse) -> Option<CachedRender> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let mime = resp.output_mime.clone().unwrap_or_default();
    if let Some(b64) = &resp.output_base64 {
        let bytes = STANDARD.decode(b64).ok()?;
        return Some(CachedRender {
            bytes,
            content_type: mime,
            input_type: resp.input_type.clone(),
        });
    }
    if let Some(text) = &resp.content {
        return Some(CachedRender {
            bytes: text.clone().into_bytes(),
            content_type: mime,
            input_type: resp.input_type.clone(),
        });
    }
    None
}

/// Inverse of [`response_to_cache_entry`] — reconstruct a response from a
/// cached entry. Sets `cached: true` so callers can tell on the wire.
fn response_from_cached(output: &str, cached: &CachedRender, mark_cached: bool) -> ConvertResponse {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let is_binary = BINARY_OUTPUTS.contains(&output);
    let mime = if cached.content_type.is_empty() {
        output_mime(output).to_string()
    } else {
        cached.content_type.clone()
    };
    if is_binary {
        let encoded = STANDARD.encode(&cached.bytes);
        ConvertResponse {
            ok: true,
            output_type: output.to_string(),
            input_type: cached.input_type.clone(),
            content: None,
            pdf_base64: if output == "pdf" {
                Some(encoded.clone())
            } else {
                None
            },
            output_base64: Some(encoded),
            output_mime: Some(mime),
            cached: mark_cached,
            warnings: Vec::new(),
        }
    } else {
        ConvertResponse {
            ok: true,
            output_type: output.to_string(),
            input_type: cached.input_type.clone(),
            content: Some(String::from_utf8_lossy(&cached.bytes).into_owned()),
            pdf_base64: None,
            output_base64: None,
            output_mime: Some(mime),
            cached: mark_cached,
            warnings: Vec::new(),
        }
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
    include_mermaid: bool,
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

    // Mermaid scripts are appended verbatim to the document. The bundle is
    // ~3 MB — heavy, but only emitted when the user actually has a
    // ```mermaid block, and avoids any third-party CDN.
    let mermaid_block = if include_mermaid {
        format!(
            "<script>{bundle}</script><script>{init}</script>",
            bundle = crate::enrichments::MERMAID_JS,
            init = crate::enrichments::MERMAID_INIT,
        )
    } else {
        String::new()
    };

    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"/>
<title>{title}</title>
<style>{theme_css}{header_footer_css}{cover_css}{page_css}{custom}</style>
</head><body>{header}{cover}{body}{footer}{mermaid_block}</body></html>"#,
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
        mermaid_block = mermaid_block,
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

static IMAGE_URL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"/api/images/(\d+)").unwrap());

/// Replace every `/api/images/{id}` substring in `document` with a
/// `data:` URI by loading the bytes from SQLite. References to images
/// that don't belong to `user` (or don't exist) are left alone — Chromium
/// will simply fail to load them and the PDF will render with the broken
/// image marker.
async fn inline_user_images(document: &str, pool: &sqlx::SqlitePool, user: &User) -> String {
    // Collect unique ids first so we don't run the same query repeatedly
    // for the same image referenced from multiple `<img>` tags.
    let mut ids: Vec<i64> = IMAGE_URL_RE
        .captures_iter(document)
        .filter_map(|c| c.get(1).and_then(|m| m.as_str().parse::<i64>().ok()))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return document.to_string();
    }

    let mut replacements: std::collections::HashMap<i64, String> =
        std::collections::HashMap::new();
    for id in ids {
        if let Ok((meta, bytes)) = images::fetch(pool, user, id).await {
            replacements.insert(id, images::to_data_uri(&meta.content_type, &bytes));
        }
    }

    IMAGE_URL_RE
        .replace_all(document, |caps: &regex::Captures| {
            caps.get(1)
                .and_then(|m| m.as_str().parse::<i64>().ok())
                .and_then(|id| replacements.get(&id).cloned())
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned()
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
        let doc = wrap_document("t", "<p>x</p>", "github", None, None, false);
        assert!(doc.contains("border-bottom: 1px solid #d1d9e0"));
    }

    #[test]
    fn wrap_document_appends_mermaid_when_requested() {
        let doc = wrap_document("t", "<pre class=\"mermaid\">graph</pre>", "default", None, None, true);
        // The bundle is large; check for a stable marker that the script tag was emitted.
        assert!(doc.contains("<script>"));
        assert!(doc.contains("mermaid.run"));
    }

    #[test]
    fn wrap_document_skips_mermaid_when_not_requested() {
        let doc = wrap_document("t", "<p>x</p>", "default", None, None, false);
        assert!(!doc.contains("mermaid.run"));
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
