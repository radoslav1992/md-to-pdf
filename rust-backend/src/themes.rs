//! Theme presets for the wrapped HTML document.
//!
//! Each theme returns a CSS string that is injected into the `<head>` of the
//! rendered document. The `default` theme is the original style and is
//! available to all users; the rest are premium.

const DEFAULT_CSS: &str = r#"
  body { font-family: -apple-system, system-ui, sans-serif; max-width: 760px; margin: 2rem auto; padding: 0 1rem; line-height: 1.55; color: #0f172a; }
  pre { background: #0f172a; color: #e2e8f0; padding: 1rem; border-radius: 8px; overflow-x: auto; }
  code { font-family: ui-monospace, monospace; font-size: 0.92em; }
  h1, h2, h3 { color: #1e3a8a; }
  a { color: #2563eb; }
  table { border-collapse: collapse; width: 100%; }
  th, td { border: 1px solid #cbd5e1; padding: 6px 10px; text-align: left; }
"#;

const CLEAN_CSS: &str = r#"
  body { font-family: 'Inter', -apple-system, system-ui, sans-serif; max-width: 780px; margin: 3rem auto; padding: 0 1.5rem; line-height: 1.7; color: #1f2937; font-size: 16px; }
  h1, h2, h3, h4 { color: #111827; font-weight: 700; letter-spacing: -0.01em; margin-top: 2em; }
  h1 { font-size: 2.25rem; border-bottom: 1px solid #e5e7eb; padding-bottom: 0.4em; }
  h2 { font-size: 1.5rem; }
  p { margin: 1em 0; }
  pre { background: #f8fafc; color: #0f172a; padding: 1rem 1.25rem; border-radius: 6px; border: 1px solid #e2e8f0; overflow-x: auto; font-size: 0.875em; }
  code { font-family: ui-monospace, 'SF Mono', monospace; font-size: 0.9em; }
  :not(pre) > code { background: #f1f5f9; padding: 0.1em 0.35em; border-radius: 3px; }
  blockquote { border-left: 4px solid #cbd5e1; margin: 1.5em 0; padding: 0.2em 1em; color: #475569; }
  a { color: #2563eb; text-decoration: none; }
  a:hover { text-decoration: underline; }
  table { border-collapse: collapse; width: 100%; margin: 1em 0; }
  th, td { border-bottom: 1px solid #e2e8f0; padding: 0.6em 0.9em; text-align: left; }
  th { background: #f8fafc; font-weight: 600; }
  img { max-width: 100%; height: auto; }
"#;

const ACADEMIC_CSS: &str = r#"
  body { font-family: 'Georgia', 'Times New Roman', serif; max-width: 720px; margin: 3rem auto; padding: 0 1.5rem; line-height: 1.75; color: #000; font-size: 12pt; text-align: justify; hyphens: auto; }
  h1, h2, h3, h4 { font-family: 'Georgia', serif; color: #000; font-weight: 700; margin-top: 1.5em; }
  h1 { font-size: 1.8em; text-align: center; margin-bottom: 0.5em; }
  h2 { font-size: 1.4em; }
  h3 { font-size: 1.15em; font-style: italic; }
  p { margin: 0.4em 0; text-indent: 1.5em; }
  p:first-of-type, h1 + p, h2 + p, h3 + p, blockquote + p { text-indent: 0; }
  blockquote { font-size: 0.95em; margin: 1.5em 2em; line-height: 1.5; }
  pre { background: #f6f6f6; padding: 1em; border-left: 3px solid #888; font-size: 9.5pt; overflow-x: auto; }
  code { font-family: 'Courier New', monospace; }
  a { color: #000; text-decoration: underline; }
  table { border-collapse: collapse; margin: 1.5em auto; }
  th, td { border: 1px solid #000; padding: 0.4em 0.8em; }
  th { font-weight: bold; }
  sup { font-size: 0.8em; }
"#;

const RESUME_CSS: &str = r#"
  body { font-family: 'Helvetica Neue', Arial, sans-serif; max-width: 720px; margin: 1.5rem auto; padding: 0 1.5rem; line-height: 1.45; color: #1a202c; font-size: 10.5pt; }
  h1 { font-size: 1.9em; text-align: center; margin: 0 0 0.1em; letter-spacing: 0.04em; text-transform: uppercase; font-weight: 700; }
  h1 + p { text-align: center; color: #4a5568; margin: 0 0 1em; }
  h2 { font-size: 0.95em; text-transform: uppercase; letter-spacing: 0.08em; border-bottom: 1.5px solid #2d3748; padding-bottom: 0.2em; margin: 1.4em 0 0.6em; }
  h3 { font-size: 1em; margin: 0.8em 0 0.1em; font-weight: 600; }
  h3 + p { color: #4a5568; font-style: italic; font-size: 0.95em; margin: 0 0 0.3em; }
  ul { margin: 0.3em 0 0.6em; padding-left: 1.2em; }
  li { margin: 0.15em 0; }
  p { margin: 0.4em 0; }
  a { color: #2b6cb0; text-decoration: none; }
  strong { font-weight: 600; }
  hr { border: none; border-top: 1px solid #cbd5e1; margin: 1em 0; }
"#;

const LETTER_CSS: &str = r#"
  body { font-family: 'Georgia', 'Times New Roman', serif; max-width: 680px; margin: 2.5rem auto; padding: 0 1.5rem; line-height: 1.6; color: #1a202c; font-size: 11.5pt; }
  h1 { font-size: 1.4em; margin: 0 0 0.3em; }
  h2 { font-size: 1.15em; }
  p { margin: 0.9em 0; }
  blockquote { margin: 1em 2em; font-style: italic; color: #4a5568; }
  hr { border: none; border-top: 1px solid #2d3748; margin: 2em 0; }
  table { border-collapse: collapse; }
  th, td { padding: 0.3em 0.8em; }
  a { color: #2b6cb0; }
"#;

const GITHUB_CSS: &str = r#"
  body { font-family: -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif; max-width: 980px; margin: 2rem auto; padding: 0 1.5rem; line-height: 1.5; color: #1f2328; font-size: 16px; }
  h1, h2, h3, h4, h5, h6 { margin-top: 24px; margin-bottom: 16px; font-weight: 600; line-height: 1.25; }
  h1 { font-size: 2em; border-bottom: 1px solid #d1d9e0; padding-bottom: 0.3em; }
  h2 { font-size: 1.5em; border-bottom: 1px solid #d1d9e0; padding-bottom: 0.3em; }
  h3 { font-size: 1.25em; }
  p { margin: 0 0 16px; }
  a { color: #0969da; text-decoration: none; }
  a:hover { text-decoration: underline; }
  pre { background: #f6f8fa; padding: 16px; border-radius: 6px; overflow-x: auto; font-size: 85%; line-height: 1.45; }
  :not(pre) > code { background: rgba(175,184,193,0.2); padding: 0.2em 0.4em; border-radius: 6px; font-size: 85%; font-family: ui-monospace, 'SF Mono', monospace; }
  pre code { background: transparent; padding: 0; font-size: inherit; }
  blockquote { padding: 0 1em; color: #59636e; border-left: 0.25em solid #d1d9e0; margin: 0 0 16px; }
  table { border-collapse: collapse; margin: 0 0 16px; display: block; overflow-x: auto; }
  th, td { padding: 6px 13px; border: 1px solid #d1d9e0; }
  th { background: #f6f8fa; font-weight: 600; }
  tr:nth-child(2n) { background: #f6f8fa; }
  ul, ol { padding-left: 2em; margin: 0 0 16px; }
  hr { border: none; border-top: 1px solid #d1d9e0; margin: 24px 0; }
  img { max-width: 100%; }
"#;

/// Premium-only themes — these unlock for paying users only.
pub const PREMIUM_THEMES: &[&str] = &["clean", "academic", "resume", "letter", "github"];

/// Return the CSS for a named theme. Unknown names fall back to `default`.
pub fn css_for(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "clean" => CLEAN_CSS,
        "academic" => ACADEMIC_CSS,
        "resume" => RESUME_CSS,
        "letter" => LETTER_CSS,
        "github" => GITHUB_CSS,
        _ => DEFAULT_CSS,
    }
}

pub fn is_premium_theme(name: &str) -> bool {
    PREMIUM_THEMES.contains(&name.to_ascii_lowercase().as_str())
}
