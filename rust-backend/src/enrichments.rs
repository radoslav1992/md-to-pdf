//! Premium HTML enrichments applied after the input format has been
//! converted to HTML and before sanitization:
//!
//! - **Table of contents** — scans `<h1>`..`<h4>`, slugifies them, and
//!   prepends a `<nav class="toc">` block linking to each heading.
//! - **Syntax highlighting** — re-renders `<pre><code class="language-…">`
//!   blocks with `syntect` so colors are baked in as inline styles
//!   (works offline in PDF output, no client-side highlighter needed).
//! - **Math** — replaces `$$…$$` and inline `$…$` LaTeX with MathML via
//!   `latex2mathml`. Chromium renders MathML natively, so this works
//!   without injecting any JS.

use once_cell::sync::Lazy;
use regex::Regex;
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

#[derive(Debug, Default, Clone, Copy)]
pub struct Enrichments {
    pub toc: bool,
    pub toc_depth: u8,
    pub syntax_highlight: bool,
    pub math: bool,
}

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

static HEADING_RE: Lazy<Regex> = Lazy::new(|| {
    // The `regex` crate doesn't support backreferences, so we match
    // either tag-number and then check level equality at substitution
    // time. The inner body allows nested tags but stops at the next h-tag.
    Regex::new(r#"(?s)<h([1-4])(?:\s+[^>]*)?>(.*?)</h([1-4])>"#).unwrap()
});

static CODE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)<pre[^>]*><code\s+class="language-([A-Za-z0-9_+\-]+)"[^>]*>(.*?)</code></pre>"#)
        .unwrap()
});

static MATH_BLOCK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)\$\$(.+?)\$\$").unwrap());
static MATH_INLINE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\$([^\$\n]+?)\$").unwrap());

pub fn apply(html: &str, options: &Enrichments) -> String {
    let mut out = html.to_string();
    if options.syntax_highlight {
        out = highlight_code_blocks(&out);
    }
    if options.math {
        out = render_math(&out);
    }
    if options.toc {
        out = prepend_toc(&out, options.toc_depth.max(1).min(4));
    }
    out
}

fn highlight_code_blocks(html: &str) -> String {
    let theme = THEME_SET
        .themes
        .get("InspiredGitHub")
        .or_else(|| THEME_SET.themes.values().next())
        .expect("at least one theme bundled");

    let mut prelude = String::new();
    if let Ok(css) = syntect::html::css_for_theme_with_class_style(theme, ClassStyle::Spaced) {
        prelude.push_str("<style>");
        prelude.push_str(&css);
        prelude.push_str(" pre.syntect { padding: 1em; overflow-x: auto; border-radius: 6px; }</style>");
    }

    let replaced = CODE_RE.replace_all(html, |caps: &regex::Captures| {
        let lang = &caps[1];
        let body = html_unescape(&caps[2]);
        let syntax = SYNTAX_SET
            .find_syntax_by_token(lang)
            .or_else(|| SYNTAX_SET.find_syntax_by_extension(lang))
            .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

        let mut generator =
            ClassedHTMLGenerator::new_with_class_style(syntax, &SYNTAX_SET, ClassStyle::Spaced);
        for line in LinesWithEndings::from(&body) {
            if generator.parse_html_for_line_which_includes_newline(line).is_err() {
                return caps[0].to_string();
            }
        }
        format!(
            "<pre class=\"syntect language-{lang}\"><code>{html}</code></pre>",
            html = generator.finalize()
        )
    });

    if prelude.is_empty() {
        replaced.into_owned()
    } else {
        let mut s = String::with_capacity(prelude.len() + replaced.len());
        s.push_str(&prelude);
        s.push_str(&replaced);
        s
    }
}

fn render_math(html: &str) -> String {
    let block_replaced = MATH_BLOCK_RE.replace_all(html, |caps: &regex::Captures| {
        let latex = html_unescape(&caps[1]);
        match latex2mathml::latex_to_mathml(&latex, latex2mathml::DisplayStyle::Block) {
            Ok(mathml) => mathml,
            Err(_) => caps[0].to_string(),
        }
    });
    MATH_INLINE_RE
        .replace_all(&block_replaced, |caps: &regex::Captures| {
            let latex = html_unescape(&caps[1]);
            match latex2mathml::latex_to_mathml(&latex, latex2mathml::DisplayStyle::Inline) {
                Ok(mathml) => mathml,
                Err(_) => caps[0].to_string(),
            }
        })
        .into_owned()
}

fn prepend_toc(html: &str, depth: u8) -> String {
    let mut entries: Vec<(u8, String, String)> = Vec::new();
    let with_ids = HEADING_RE.replace_all(html, |caps: &regex::Captures| {
        let open_level: u8 = caps[1].parse().unwrap_or(1);
        let close_level: u8 = caps[3].parse().unwrap_or(0);
        if open_level != close_level {
            return caps[0].to_string();
        }
        if open_level > depth {
            return caps[0].to_string();
        }
        let text = strip_tags(&caps[2]).trim().to_string();
        if text.is_empty() {
            return caps[0].to_string();
        }
        let slug = slugify(&text, &entries);
        entries.push((open_level, slug.clone(), text.clone()));
        format!(
            "<h{level} id=\"{slug}\">{body}</h{level}>",
            level = open_level,
            slug = slug,
            body = caps[2].trim(),
        )
    });
    if entries.is_empty() {
        return with_ids.into_owned();
    }

    let mut toc = String::from("<nav class=\"toc\"><div class=\"toc-title\">Contents</div><ul>");
    for (level, slug, text) in &entries {
        toc.push_str(&format!(
            "<li class=\"toc-l{level}\"><a href=\"#{slug}\">{text}</a></li>",
            level = level,
            slug = slug,
            text = html_escape(text),
        ));
    }
    toc.push_str("</ul></nav>");

    let style = "<style>\
        nav.toc { border: 1px solid #e5e7eb; border-radius: 6px; padding: 1em 1.4em; margin: 0 0 2em; background: #f8fafc; page-break-after: always; }\
        nav.toc .toc-title { font-weight: 700; font-size: 1.1em; margin-bottom: 0.4em; }\
        nav.toc ul { list-style: none; padding: 0; margin: 0; }\
        nav.toc li { padding: 0.15em 0; }\
        nav.toc .toc-l2 { padding-left: 1.2em; }\
        nav.toc .toc-l3 { padding-left: 2.4em; font-size: 0.95em; }\
        nav.toc .toc-l4 { padding-left: 3.6em; font-size: 0.9em; color: #475569; }\
        nav.toc a { color: inherit; text-decoration: none; }\
        nav.toc a:hover { text-decoration: underline; }\
        </style>";

    let mut out = String::with_capacity(style.len() + toc.len() + with_ids.len());
    out.push_str(style);
    out.push_str(&toc);
    out.push_str(&with_ids);
    out
}

fn slugify(text: &str, existing: &[(u8, String, String)]) -> String {
    let base: String = text
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else if c.is_whitespace() || c == '-' || c == '_' {
                '-'
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let base = collapse_dashes(if base.is_empty() { "section" } else { &base });

    let mut candidate = base.clone();
    let mut suffix = 2;
    while existing.iter().any(|(_, s, _)| *s == candidate) {
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
    candidate
}

fn collapse_dashes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_dash = false;
    for c in input.chars() {
        if c == '-' {
            if !prev_dash {
                out.push('-');
            }
            prev_dash = true;
        } else {
            out.push(c);
            prev_dash = false;
        }
    }
    out
}

fn strip_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for c in input.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            other if !in_tag => out.push(other),
            _ => {}
        }
    }
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

fn html_unescape(input: &str) -> String {
    input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toc_adds_ids_and_nav() {
        let html = "<h1>Intro</h1><p>x</p><h2>Setup</h2><h2>Setup</h2>";
        let out = prepend_toc(html, 4);
        assert!(out.contains("<nav class=\"toc\">"));
        assert!(out.contains("id=\"intro\""));
        assert!(out.contains("id=\"setup\""));
        assert!(out.contains("id=\"setup-2\""));
        assert!(out.contains("href=\"#intro\""));
    }

    #[test]
    fn toc_respects_depth() {
        let html = "<h1>A</h1><h2>B</h2><h3>C</h3>";
        let out = prepend_toc(html, 2);
        assert!(out.contains("href=\"#a\""));
        assert!(out.contains("href=\"#b\""));
        assert!(!out.contains("href=\"#c\""));
    }

    #[test]
    fn syntax_highlight_wraps_in_spans() {
        let html = r#"<pre><code class="language-rust">fn main() {}</code></pre>"#;
        let out = highlight_code_blocks(html);
        assert!(out.contains("class=\"syntect"));
        assert!(out.contains("<span"));
    }

    #[test]
    fn syntax_highlight_unknown_language_falls_back_to_plain() {
        let html = r#"<pre><code class="language-zzz">hello</code></pre>"#;
        let out = highlight_code_blocks(html);
        assert!(out.contains("hello"));
    }

    #[test]
    fn math_block_becomes_mathml() {
        let html = "<p>$$a + b$$</p>";
        let out = render_math(html);
        assert!(out.contains("<math"));
        assert!(out.contains("display=\"block\""));
    }

    #[test]
    fn math_inline_becomes_mathml() {
        let html = "<p>see $x^2$ here</p>";
        let out = render_math(html);
        assert!(out.contains("<math"));
        assert!(!out.contains("$x^2$"));
    }

    #[test]
    fn math_passes_through_bad_latex() {
        let html = "<p>$\\not_real{$</p>";
        let out = render_math(html);
        assert!(out.contains("$"));
    }

    #[test]
    fn slugify_strips_punctuation() {
        let s = slugify("Hello, World!", &[]);
        assert_eq!(s, "hello-world");
    }
}
