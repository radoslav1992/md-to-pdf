//! `udc` — command-line client for the Universal Document Converter API.
//!
//! Configuration:
//!   - `UDC_BASE_URL` / `--base-url` (default `http://127.0.0.1:8000`)
//!   - `UDC_API_KEY`  / `--api-key`  (or saved via `udc login`)
//!
//! The saved config lives at the platform's user-config dir
//! (e.g. `~/.config/udc/config.json` on Linux) and stores the base URL
//! plus the bearer token in plaintext — the same way other API CLIs do.
//! Don't put this on shared machines.

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use clap::{Parser, Subcommand, ValueEnum};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

/// Output formats the API can emit. Mirrors the server's `output` enum.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Html,
    Pdf,
}

impl OutputFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Pdf => "pdf",
        }
    }
}

/// Input formats the API accepts.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum InputFormat {
    Markdown,
    Html,
    Json,
    Xml,
    Csv,
    Org,
    Asciidoc,
    Rst,
    Latex,
}

impl InputFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Json => "json",
            Self::Xml => "xml",
            Self::Csv => "csv",
            Self::Org => "org",
            Self::Asciidoc => "asciidoc",
            Self::Rst => "rst",
            Self::Latex => "latex",
        }
    }

    /// Guess from a path's extension. Returns `None` if unknown so the
    /// caller can fall back to an explicit `--type`.
    fn from_path(path: &std::path::Path) -> Option<Self> {
        match path.extension().and_then(|s| s.to_str())?.to_ascii_lowercase().as_str() {
            "md" | "markdown" => Some(Self::Markdown),
            "html" | "htm" => Some(Self::Html),
            "json" => Some(Self::Json),
            "xml" => Some(Self::Xml),
            "csv" => Some(Self::Csv),
            "org" => Some(Self::Org),
            "adoc" | "asciidoc" => Some(Self::Asciidoc),
            "rst" => Some(Self::Rst),
            "tex" | "latex" => Some(Self::Latex),
            _ => None,
        }
    }
}

#[derive(Parser)]
#[command(name = "udc", version, about = "Universal Document Converter CLI")]
struct Cli {
    /// API base URL. Default: $UDC_BASE_URL or http://127.0.0.1:8000.
    #[arg(long, global = true)]
    base_url: Option<String>,
    /// API key. Default: $UDC_API_KEY or the saved login.
    #[arg(long, global = true)]
    api_key: Option<String>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Convert a file (or stdin) to HTML or PDF.
    Convert {
        /// Path to read input from. Use `-` for stdin.
        input: String,
        /// Output path. Use `-` for stdout (text formats only).
        #[arg(short, long, default_value = "-")]
        output: String,
        /// Override the input format. Guessed from the extension if omitted.
        #[arg(short, long = "type", value_enum)]
        input_type: Option<InputFormat>,
        /// Output format. Default: pdf.
        #[arg(short = 'f', long = "format", value_enum, default_value_t = OutputFormat::Pdf)]
        format: OutputFormat,
        /// Theme. Default theme is free; the rest need a premium key.
        #[arg(long)]
        theme: Option<String>,
        /// Document title (shows in the PDF metadata + cover).
        #[arg(long)]
        title: Option<String>,
    },
    /// Extract text from a PDF (premium).
    Extract {
        /// Path to a PDF file.
        input: String,
        /// Output path; defaults to stdout.
        #[arg(short, long, default_value = "-")]
        output: String,
        /// Force OCR even if the PDF has a text layer.
        #[arg(long)]
        force_ocr: bool,
    },
    /// Print the current usage summary.
    Usage,
    /// List your URL watches.
    Watches,
    /// Save the base URL + API key so future commands don't need flags.
    Login {
        /// API base URL to persist.
        base_url: String,
        /// API key to persist. Pass `-` to read from stdin.
        api_key: String,
    },
    /// Forget any saved credentials.
    Logout,
    /// Print where the config file lives.
    Config,
    /// Hit /api/v1/health.
    Health,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Config {
    base_url: Option<String>,
    api_key: Option<String>,
}

fn config_path() -> Result<PathBuf> {
    let proj = ProjectDirs::from("com", "universal-converter", "udc")
        .ok_or_else(|| anyhow!("could not resolve user config directory"))?;
    fs::create_dir_all(proj.config_dir()).ok();
    Ok(proj.config_dir().join("config.json"))
}

fn load_config() -> Config {
    let Ok(p) = config_path() else { return Config::default(); };
    let Ok(s) = fs::read_to_string(p) else { return Config::default(); };
    serde_json::from_str(&s).unwrap_or_default()
}

fn save_config(cfg: &Config) -> Result<PathBuf> {
    let p = config_path()?;
    fs::write(&p, serde_json::to_vec_pretty(cfg)?)?;
    // Best-effort: tighten permissions on POSIX so the key isn't world-
    // readable. Errors are non-fatal.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    Ok(p)
}

struct Client {
    base: String,
    key: Option<String>,
    http: reqwest::blocking::Client,
}

impl Client {
    fn from_cli(cli: &Cli) -> Result<Self> {
        let saved = load_config();
        let base = cli
            .base_url
            .clone()
            .or_else(|| std::env::var("UDC_BASE_URL").ok())
            .or(saved.base_url)
            .unwrap_or_else(|| "http://127.0.0.1:8000".to_string());
        let key = cli
            .api_key
            .clone()
            .or_else(|| std::env::var("UDC_API_KEY").ok())
            .or(saved.api_key);
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            key,
            http,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base, path)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::blocking::RequestBuilder {
        let mut r = self.http.request(method, self.url(path));
        if let Some(k) = &self.key {
            r = r.bearer_auth(k);
        }
        r
    }

    /// Submit a JSON request, return the deserialised JSON on 2xx, or
    /// surface the server's `error` string on failure.
    fn post_json<T: serde::Serialize>(&self, path: &str, body: &T) -> Result<serde_json::Value> {
        let resp = self
            .request(reqwest::Method::POST, path)
            .json(body)
            .send()
            .with_context(|| format!("POST {}", self.url(path)))?;
        let status = resp.status();
        let text = resp.text()?;
        let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|_| {
            serde_json::json!({ "ok": false, "error": text.clone() })
        });
        if !status.is_success() {
            let err = value
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or(&text)
                .to_string();
            bail!("{status}: {err}");
        }
        Ok(value)
    }

    fn get_json(&self, path: &str) -> Result<serde_json::Value> {
        let resp = self
            .request(reqwest::Method::GET, path)
            .send()
            .with_context(|| format!("GET {}", self.url(path)))?;
        let status = resp.status();
        let text = resp.text()?;
        if !status.is_success() {
            bail!("{status}: {text}");
        }
        Ok(serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)))
    }
}

fn read_input(path: &str) -> Result<String> {
    if path == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        Ok(s)
    } else {
        Ok(fs::read_to_string(path)?)
    }
}

fn write_output(path: &str, bytes: &[u8], binary: bool) -> Result<()> {
    if path == "-" {
        if binary {
            std::io::stdout().write_all(bytes)?;
        } else {
            std::io::stdout().write_all(bytes)?;
        }
    } else {
        fs::write(path, bytes)?;
    }
    Ok(())
}

fn cmd_convert(
    client: &Client,
    input: &str,
    output: &str,
    explicit_type: Option<InputFormat>,
    format: OutputFormat,
    theme: Option<&str>,
    title: Option<&str>,
) -> Result<()> {
    let input_type = explicit_type
        .or_else(|| InputFormat::from_path(std::path::Path::new(input)))
        .ok_or_else(|| anyhow!("could not infer input type from {input}; pass --type"))?;
    let content = read_input(input)?;

    let mut body = serde_json::json!({
        "type": input_type.as_str(),
        "output": format.as_str(),
        "content": content,
    });
    if let Some(t) = title {
        body["title"] = serde_json::Value::String(t.into());
    }
    if let Some(t) = theme {
        body["theme"] = serde_json::Value::String(t.into());
    }

    let resp = client.post_json("/convert", &body)?;
    match format {
        OutputFormat::Html => {
            let html = resp
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("server returned no `content`"))?;
            write_output(output, html.as_bytes(), false)?;
        }
        OutputFormat::Pdf => {
            // Prefer the v1 `output_base64`, fall back to legacy `pdf_base64`.
            let b64 = resp
                .get("output_base64")
                .and_then(|v| v.as_str())
                .or_else(|| resp.get("pdf_base64").and_then(|v| v.as_str()))
                .ok_or_else(|| anyhow!("server returned no PDF bytes"))?;
            let bytes = STANDARD.decode(b64).context("decode pdf bytes")?;
            if output == "-" {
                // Writing binary PDF to stdout when stdout is a TTY is
                // almost always a mistake — refuse to make a mess.
                if atty_stdout() {
                    bail!("refusing to write PDF to a terminal; pass -o file.pdf");
                }
            }
            write_output(output, &bytes, true)?;
        }
    }
    Ok(())
}

/// Tiny stand-in for the `atty` crate so we don't add a dep for one check.
fn atty_stdout() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        // SAFETY: passing a valid fd from stdout to libc::isatty.
        unsafe { libc_isatty(std::io::stdout().as_raw_fd()) }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
unsafe fn libc_isatty(fd: std::os::unix::io::RawFd) -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    isatty(fd) != 0
}

fn cmd_extract(client: &Client, input: &str, output: &str, force_ocr: bool) -> Result<()> {
    let mut bytes = Vec::new();
    if input == "-" {
        std::io::stdin().read_to_end(&mut bytes)?;
    } else {
        bytes = fs::read(input)?;
    }
    let body = serde_json::json!({
        "pdf_base64": STANDARD.encode(&bytes),
        "ocr": if force_ocr { "force" } else { "auto" },
    });
    let resp = client.post_json("/extract", &body)?;
    let md = resp
        .get("markdown")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("server returned no `markdown`"))?;
    write_output(output, md.as_bytes(), false)?;
    Ok(())
}

fn cmd_usage(client: &Client) -> Result<()> {
    let r = client.get_json("/usage")?;
    println!("{}", serde_json::to_string_pretty(&r)?);
    Ok(())
}

fn cmd_watches(client: &Client) -> Result<()> {
    let r = client.get_json("/url-watches")?;
    println!("{}", serde_json::to_string_pretty(&r)?);
    Ok(())
}

fn cmd_login(base_url: &str, api_key: &str) -> Result<()> {
    let key = if api_key == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        s.trim().to_string()
    } else {
        api_key.to_string()
    };
    if key.is_empty() {
        bail!("api key is empty");
    }
    let cfg = Config {
        base_url: Some(base_url.trim_end_matches('/').to_string()),
        api_key: Some(key),
    };
    let p = save_config(&cfg)?;
    eprintln!("saved credentials to {}", p.display());
    Ok(())
}

fn cmd_logout() -> Result<()> {
    if let Ok(p) = config_path() {
        if p.exists() {
            fs::remove_file(&p)?;
            eprintln!("removed {}", p.display());
        }
    }
    Ok(())
}

fn cmd_config() -> Result<()> {
    let p = config_path()?;
    println!("{}", p.display());
    Ok(())
}

fn cmd_health(client: &Client) -> Result<()> {
    let r = client.get_json("/health")?;
    println!("{}", serde_json::to_string_pretty(&r)?);
    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    match &cli.command {
        Cmd::Convert {
            input,
            output,
            input_type,
            format,
            theme,
            title,
        } => {
            let client = Client::from_cli(&cli)?;
            cmd_convert(
                &client,
                input,
                output,
                *input_type,
                *format,
                theme.as_deref(),
                title.as_deref(),
            )
        }
        Cmd::Extract {
            input,
            output,
            force_ocr,
        } => {
            let client = Client::from_cli(&cli)?;
            cmd_extract(&client, input, output, *force_ocr)
        }
        Cmd::Usage => cmd_usage(&Client::from_cli(&cli)?),
        Cmd::Watches => cmd_watches(&Client::from_cli(&cli)?),
        Cmd::Login { base_url, api_key } => cmd_login(base_url, api_key),
        Cmd::Logout => cmd_logout(),
        Cmd::Config => cmd_config(),
        Cmd::Health => cmd_health(&Client::from_cli(&cli)?),
    }
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli) {
        // Print the chain inline (no backtrace) so the user sees
        // `udc: 401 Unauthorized` rather than a Rust panic dump.
        eprint!("udc: {err}");
        let mut source = err.source();
        while let Some(cause) = source {
            eprint!("\n  caused by: {cause}");
            source = cause.source();
        }
        eprintln!();
        std::process::exit(1);
    }
}
