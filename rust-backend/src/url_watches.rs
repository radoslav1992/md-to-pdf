//! URL watches: convert-when-it-changes pipelines.
//!
//! A user creates a watch over an HTTP(S) URL plus a delivery target.
//! A background `tokio::spawn`'d worker (`run_worker`) wakes every
//! `WORKER_TICK_SECS`, finds watches whose `last_polled_at +
//! poll_interval_secs` has passed, fetches the URL, hashes the body,
//! and — if the hash changed since the previous poll — runs a
//! conversion and POSTs the result to `target_url` (signed if
//! `target_secret` is set).
//!
//! This is the inverse of the batch webhook: there the caller pushes
//! work and we deliver results to a URL; here we *poll* a URL ourselves
//! and turn its content into work.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use std::sync::Arc;
use std::time::Duration;

use crate::convert;
use crate::db::{now_seconds, AppState, User};
use crate::error::ConvertError;

/// How often the worker wakes. The actual poll cadence for any one
/// watch is governed by its `poll_interval_secs`; the worker just
/// checks who's due.
const WORKER_TICK_SECS: u64 = 30;

/// Floor on what users can set as `poll_interval_secs`. Anything lower
/// would let a malicious or careless watcher hammer the source URL.
const MIN_POLL_INTERVAL_SECS: i64 = 60;
/// Ceiling — anything longer is basically a manual job.
const MAX_POLL_INTERVAL_SECS: i64 = 7 * 24 * 60 * 60;

/// Hard cap on the fetched body size, in bytes. Keeps a watch pointed
/// at a giant binary from blowing up RAM. Conversions of larger inputs
/// can still be triggered through `/convert` directly.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

const HTTP_TIMEOUT_SECS: u64 = 30;
const NAME_MAX: usize = 80;
const URL_MAX: usize = 1024;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct UrlWatch {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub url: String,
    pub input_type: String,
    pub output_format: String,
    pub target_url: String,
    /// Hidden from `Public` serializer below — clients see whether a
    /// secret is set but never read it back.
    #[serde(skip)]
    pub target_secret: Option<String>,
    pub poll_interval_secs: i64,
    pub enabled: i64,
    pub last_seen_hash: Option<String>,
    pub last_seen_at: Option<i64>,
    pub last_polled_at: Option<i64>,
    pub last_delivered_at: Option<i64>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicWatch {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub url: String,
    pub input_type: String,
    pub output_format: String,
    pub target_url: String,
    pub has_secret: bool,
    pub poll_interval_secs: i64,
    pub enabled: bool,
    pub last_seen_at: Option<i64>,
    pub last_polled_at: Option<i64>,
    pub last_delivered_at: Option<i64>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<&UrlWatch> for PublicWatch {
    fn from(w: &UrlWatch) -> Self {
        PublicWatch {
            id: w.id,
            user_id: w.user_id,
            name: w.name.clone(),
            url: w.url.clone(),
            input_type: w.input_type.clone(),
            output_format: w.output_format.clone(),
            target_url: w.target_url.clone(),
            has_secret: w.target_secret.as_deref().is_some_and(|s| !s.is_empty()),
            poll_interval_secs: w.poll_interval_secs,
            enabled: w.enabled != 0,
            last_seen_at: w.last_seen_at,
            last_polled_at: w.last_polled_at,
            last_delivered_at: w.last_delivered_at,
            last_error: w.last_error.clone(),
            created_at: w.created_at,
            updated_at: w.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SaveWatch {
    pub name: String,
    pub url: String,
    pub input_type: String,
    pub output_format: String,
    pub target_url: String,
    #[serde(default)]
    pub target_secret: Option<String>,
    #[serde(default)]
    pub poll_interval_secs: Option<i64>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

const SELECT: &str = "id, user_id, name, url, input_type, output_format, target_url, \
     target_secret, poll_interval_secs, enabled, last_seen_hash, last_seen_at, \
     last_polled_at, last_delivered_at, last_error, created_at, updated_at";

fn validate_payload(p: &SaveWatch) -> Result<i64, ConvertError> {
    let name = p.name.trim();
    if name.is_empty() || name.len() > NAME_MAX {
        return Err(ConvertError::BadRequest(
            "name must be 1..=80 characters".into(),
        ));
    }
    validate_http_url(&p.url, "url")?;
    validate_http_url(&p.target_url, "target_url")?;
    validate_input_type(&p.input_type)?;
    validate_output_format(&p.output_format)?;
    let interval = p.poll_interval_secs.unwrap_or(900);
    if !(MIN_POLL_INTERVAL_SECS..=MAX_POLL_INTERVAL_SECS).contains(&interval) {
        return Err(ConvertError::BadRequest(format!(
            "poll_interval_secs must be {MIN_POLL_INTERVAL_SECS}..={MAX_POLL_INTERVAL_SECS}"
        )));
    }
    Ok(interval)
}

fn validate_http_url(s: &str, field: &str) -> Result<(), ConvertError> {
    let s = s.trim();
    if s.is_empty() || s.len() > URL_MAX {
        return Err(ConvertError::BadRequest(format!(
            "{field} must be 1..=1024 characters"
        )));
    }
    if !(s.starts_with("http://") || s.starts_with("https://")) {
        return Err(ConvertError::BadRequest(format!(
            "{field} must be an http(s) URL"
        )));
    }
    Ok(())
}

fn validate_input_type(s: &str) -> Result<(), ConvertError> {
    match s {
        "markdown" | "md" | "html" | "json" | "xml" | "csv" | "org" | "asciidoc" | "adoc"
        | "rst" | "latex" | "tex" => Ok(()),
        other => Err(ConvertError::BadRequest(format!(
            "unsupported input_type: {other}"
        ))),
    }
}

fn validate_output_format(s: &str) -> Result<(), ConvertError> {
    match s {
        "html" | "pdf" => Ok(()),
        other => Err(ConvertError::BadRequest(format!(
            "unsupported output_format: {other} (only html or pdf for watches)"
        ))),
    }
}

pub async fn list(pool: &SqlitePool, user: &User) -> Result<Vec<PublicWatch>, ConvertError> {
    let rows: Vec<UrlWatch> = sqlx::query_as(&format!(
        "SELECT {SELECT} FROM url_watches WHERE user_id = ?1 ORDER BY created_at DESC LIMIT 200"
    ))
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(PublicWatch::from).collect())
}

pub async fn get(pool: &SqlitePool, user: &User, id: i64) -> Result<PublicWatch, ConvertError> {
    let row: Option<UrlWatch> = sqlx::query_as(&format!(
        "SELECT {SELECT} FROM url_watches WHERE id = ?1 AND user_id = ?2"
    ))
    .bind(id)
    .bind(user.id)
    .fetch_optional(pool)
    .await?;
    row.map(|w| PublicWatch::from(&w)).ok_or(ConvertError::NotFound)
}

pub async fn create(
    pool: &SqlitePool,
    user: &User,
    payload: &SaveWatch,
) -> Result<PublicWatch, ConvertError> {
    if !user.is_premium() {
        return Err(ConvertError::PremiumRequired);
    }
    let interval = validate_payload(payload)?;
    let now = now_seconds();
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO url_watches \
         (user_id, name, url, input_type, output_format, target_url, target_secret, \
          poll_interval_secs, enabled, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10) RETURNING id",
    )
    .bind(user.id)
    .bind(payload.name.trim())
    .bind(payload.url.trim())
    .bind(payload.input_type.trim().to_ascii_lowercase())
    .bind(payload.output_format.trim().to_ascii_lowercase())
    .bind(payload.target_url.trim())
    .bind(payload.target_secret.as_deref())
    .bind(interval)
    .bind(if payload.enabled { 1i64 } else { 0 })
    .bind(now)
    .fetch_one(pool)
    .await?;
    get(pool, user, row.0).await
}

pub async fn update(
    pool: &SqlitePool,
    user: &User,
    id: i64,
    payload: &SaveWatch,
) -> Result<PublicWatch, ConvertError> {
    if !user.is_premium() {
        return Err(ConvertError::PremiumRequired);
    }
    let interval = validate_payload(payload)?;
    let now = now_seconds();
    let res = sqlx::query(
        "UPDATE url_watches SET name = ?1, url = ?2, input_type = ?3, output_format = ?4, \
         target_url = ?5, target_secret = ?6, poll_interval_secs = ?7, enabled = ?8, \
         updated_at = ?9 WHERE id = ?10 AND user_id = ?11",
    )
    .bind(payload.name.trim())
    .bind(payload.url.trim())
    .bind(payload.input_type.trim().to_ascii_lowercase())
    .bind(payload.output_format.trim().to_ascii_lowercase())
    .bind(payload.target_url.trim())
    .bind(payload.target_secret.as_deref())
    .bind(interval)
    .bind(if payload.enabled { 1i64 } else { 0 })
    .bind(now)
    .bind(id)
    .bind(user.id)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(ConvertError::NotFound);
    }
    get(pool, user, id).await
}

pub async fn delete(pool: &SqlitePool, user: &User, id: i64) -> Result<(), ConvertError> {
    let res = sqlx::query("DELETE FROM url_watches WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(user.id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ConvertError::NotFound);
    }
    Ok(())
}

// ---------- worker ----------

pub async fn run_worker(state: Arc<AppState>) {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .user_agent("UniversalDocumentConverter/1.0 (+watch-worker)")
        .build()
        .unwrap_or_else(|e| {
            tracing::error!(error = %e, "watch worker: failed to build http client");
            reqwest::Client::new()
        });

    tracing::info!(tick_secs = WORKER_TICK_SECS, "url watch worker started");

    loop {
        tokio::time::sleep(Duration::from_secs(WORKER_TICK_SECS)).await;
        if let Err(e) = tick(&state, &http).await {
            tracing::warn!(error = %e, "watch worker tick failed");
        }
    }
}

async fn tick(state: &AppState, http: &reqwest::Client) -> Result<(), ConvertError> {
    let now = now_seconds();
    let due: Vec<UrlWatch> = sqlx::query_as(&format!(
        "SELECT {SELECT} FROM url_watches \
         WHERE enabled = 1 AND \
               (last_polled_at IS NULL OR last_polled_at + poll_interval_secs <= ?1) \
         ORDER BY COALESCE(last_polled_at, 0) ASC LIMIT 32"
    ))
    .bind(now)
    .fetch_all(&state.pool)
    .await?;

    for watch in due {
        state.metrics.url_watch_polls_total.inc();
        if let Err(e) = process_one(state, http, &watch).await {
            tracing::warn!(watch_id = watch.id, error = %e, "watch poll failed");
            state.metrics.url_watch_failures_total.inc();
            // Record the error so the dashboard can show it.
            let _ = sqlx::query(
                "UPDATE url_watches SET last_polled_at = ?1, last_error = ?2, updated_at = ?1 \
                 WHERE id = ?3",
            )
            .bind(now_seconds())
            .bind(format!("{e}"))
            .bind(watch.id)
            .execute(&state.pool)
            .await;
        }
    }
    Ok(())
}

async fn process_one(
    state: &AppState,
    http: &reqwest::Client,
    watch: &UrlWatch,
) -> Result<(), ConvertError> {
    let poll_started = now_seconds();
    let res = http
        .get(&watch.url)
        .send()
        .await
        .map_err(|e| ConvertError::Internal(format!("fetch {}: {e}", watch.url)))?;
    if !res.status().is_success() {
        return Err(ConvertError::Internal(format!(
            "fetch {}: HTTP {}",
            watch.url,
            res.status()
        )));
    }
    let bytes = res
        .bytes()
        .await
        .map_err(|e| ConvertError::Internal(format!("read body: {e}")))?;
    if bytes.len() > MAX_BODY_BYTES {
        return Err(ConvertError::PayloadTooLarge(bytes.len(), MAX_BODY_BYTES));
    }
    let body = String::from_utf8_lossy(&bytes).into_owned();

    // Hash and compare. Unchanged content → just record poll time and bail.
    let mut h = Sha256::new();
    h.update(body.as_bytes());
    let hash = hex::encode(h.finalize());
    if watch.last_seen_hash.as_deref() == Some(hash.as_str()) {
        sqlx::query(
            "UPDATE url_watches SET last_polled_at = ?1, last_error = NULL, updated_at = ?1 \
             WHERE id = ?2",
        )
        .bind(poll_started)
        .bind(watch.id)
        .execute(&state.pool)
        .await?;
        return Ok(());
    }

    // Changed (or first-ever poll). Run the conversion under the
    // watch's owner so per-user image inlining & quota stay correct.
    let owner = sqlx::query_as::<_, crate::db::User>(
        "SELECT id, email, password_hash, password_salt, role, created_at \
         FROM users WHERE id = ?1",
    )
    .bind(watch.user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ConvertError::NotFound)?;

    let req = convert::ConvertRequest {
        input_type: watch.input_type.clone(),
        output: watch.output_format.clone(),
        content: body,
        files: None,
        title: Some(watch.name.clone()),
        theme: None,
        custom_css: None,
        pdf_options: None,
        template_id: None,
        enrichments: None,
    };
    let conv = convert::run(&req, state, Some(&owner)).await?;

    // Deliver. We pre-serialise the JSON body so the HMAC and the POST
    // see exactly the same bytes.
    let payload = serde_json::json!({
        "watch_id": watch.id,
        "name": watch.name,
        "url": watch.url,
        "fetched_at": poll_started,
        "input_type": conv.input_type,
        "output_type": conv.output_type,
        "content": conv.content,
        "pdf_base64": conv.pdf_base64,
    });
    let body_bytes = serde_json::to_vec(&payload)
        .map_err(|e| ConvertError::Internal(format!("serialise webhook: {e}")))?;

    let mut req = http
        .post(&watch.target_url)
        .header("content-type", "application/json")
        .header("x-udc-event", "url_watch.changed")
        .header("x-udc-watch-id", watch.id.to_string());
    if let Some(secret) = watch.target_secret.as_deref().filter(|s| !s.is_empty()) {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|e| ConvertError::Internal(format!("hmac: {e}")))?;
        mac.update(&body_bytes);
        let sig = STANDARD.encode(mac.finalize().into_bytes());
        req = req.header("x-udc-signature", sig);
    }
    let delivery = req.body(body_bytes).send().await;
    let delivered = matches!(&delivery, Ok(r) if r.status().is_success());
    if delivered {
        state.metrics.url_watch_deliveries_total.inc();
    } else {
        state.metrics.url_watch_failures_total.inc();
    }

    sqlx::query(
        "UPDATE url_watches SET last_polled_at = ?1, last_seen_at = ?1, last_seen_hash = ?2, \
         last_delivered_at = CASE WHEN ?3 = 1 THEN ?1 ELSE last_delivered_at END, \
         last_error = ?4, updated_at = ?1 WHERE id = ?5",
    )
    .bind(poll_started)
    .bind(&hash)
    .bind(if delivered { 1i64 } else { 0 })
    .bind(match delivery {
        Ok(r) if r.status().is_success() => None,
        Ok(r) => Some(format!("delivery returned HTTP {}", r.status())),
        Err(e) => Some(format!("delivery failed: {e}")),
    })
    .bind(watch.id)
    .execute(&state.pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(url: &str, target: &str) -> SaveWatch {
        SaveWatch {
            name: "x".into(),
            url: url.into(),
            input_type: "markdown".into(),
            output_format: "html".into(),
            target_url: target.into(),
            target_secret: None,
            poll_interval_secs: Some(900),
            enabled: true,
        }
    }

    #[test]
    fn validate_rejects_non_http_url() {
        let p = payload("file:///etc/passwd", "https://example.com/wh");
        assert!(validate_payload(&p).is_err());
    }

    #[test]
    fn validate_rejects_fast_poll() {
        let mut p = payload("https://a", "https://b");
        p.poll_interval_secs = Some(5);
        assert!(validate_payload(&p).is_err());
    }

    #[test]
    fn validate_rejects_unknown_input() {
        let mut p = payload("https://a", "https://b");
        p.input_type = "docx".into();
        assert!(validate_payload(&p).is_err());
    }

    #[test]
    fn validate_rejects_binary_output_for_watches() {
        let mut p = payload("https://a", "https://b");
        p.output_format = "docx".into();
        assert!(validate_payload(&p).is_err());
    }

    #[test]
    fn validate_accepts_sane_payload() {
        let p = payload(
            "https://raw.githubusercontent.com/foo/bar/main/README.md",
            "https://example.com/webhook",
        );
        assert_eq!(validate_payload(&p).unwrap(), 900);
    }
}
