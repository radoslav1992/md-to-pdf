use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use std::sync::Arc;

use crate::metrics::SharedMetrics;
use crate::rate_limit::SharedRateLimiter;
use crate::render_cache::SharedRenderCache;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub admin_emails: Vec<String>,
    pub cookie_secure: bool,
    pub chromium_bin: String,
    /// Pool of pre-created Chromium user-data dirs. Acquiring a slot is
    /// how concurrent renders are gated and how warm caches survive.
    pub chromium_pool: Arc<crate::pdf::ChromiumSlotPool>,
    pub render_cache: SharedRenderCache,
    /// Per-identity token bucket rate limiter. Wired in via
    /// `rate_limit_middleware` on the `/api/...` and `/api/v1/...`
    /// router stacks.
    pub rate_limiter: SharedRateLimiter,
    /// In-process metrics, exported at `/metrics`. The same `Arc` is
    /// shared by every handler, the request-timing middleware, and
    /// background workers.
    pub metrics: SharedMetrics,
}

pub async fn connect(url: &str) -> Result<SqlitePool, sqlx::Error> {
    let opts = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));

    SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    #[serde(skip_serializing)]
    pub password_salt: String,
    pub role: String,
    pub created_at: i64,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }

    pub fn is_premium(&self) -> bool {
        self.role == "premium" || self.role == "admin"
    }

    pub fn public(&self) -> PublicUser {
        PublicUser {
            id: self.id,
            email: self.email.clone(),
            role: self.role.clone(),
            created_at: self.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicUser {
    pub id: i64,
    pub email: String,
    pub role: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Document {
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub input_type: String,
    pub output_type: String,
    pub content: String,
    pub rendered_html: Option<String>,
    pub theme: Option<String>,
    pub custom_css: Option<String>,
    pub pdf_options: Option<String>,
    pub is_encrypted: i64,
    pub encryption_salt: Option<String>,
    pub folder: Option<String>,
    pub tags: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentSummary {
    pub id: i64,
    pub title: String,
    pub input_type: String,
    pub output_type: String,
    pub is_encrypted: bool,
    pub folder: Option<String>,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<&Document> for DocumentSummary {
    fn from(d: &Document) -> Self {
        DocumentSummary {
            id: d.id,
            title: d.title.clone(),
            input_type: d.input_type.clone(),
            output_type: d.output_type.clone(),
            is_encrypted: d.is_encrypted != 0,
            folder: d.folder.clone(),
            tags: parse_tags(d.tags.as_deref()),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

/// API representation of a single document. `Document` mirrors the
/// SQLite row 1:1 (so it has `tags: Option<String>` storing the raw
/// JSON column, and `is_encrypted: i64` for SQLite's bool-as-int),
/// which would serialise straight through to the frontend with
/// surprising types — the React code expects `tags: string[]` and
/// crashes with `n.tags.map is not a function` when it sees a string.
/// Always go through `DocumentDetail` when sending a single document
/// over the wire so the wire shape matches what the TypeScript types
/// declare.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentDetail {
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub input_type: String,
    pub output_type: String,
    pub content: String,
    pub rendered_html: Option<String>,
    pub theme: Option<String>,
    pub custom_css: Option<String>,
    pub pdf_options: Option<String>,
    pub is_encrypted: bool,
    pub encryption_salt: Option<String>,
    pub folder: Option<String>,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<Document> for DocumentDetail {
    fn from(d: Document) -> Self {
        DocumentDetail {
            id: d.id,
            user_id: d.user_id,
            title: d.title,
            input_type: d.input_type,
            output_type: d.output_type,
            content: d.content,
            rendered_html: d.rendered_html,
            theme: d.theme,
            custom_css: d.custom_css,
            pdf_options: d.pdf_options,
            is_encrypted: d.is_encrypted != 0,
            encryption_salt: d.encryption_salt,
            folder: d.folder,
            tags: parse_tags(d.tags.as_deref()),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

/// Decode the tags column (JSON array of strings) into a Vec. Returns
/// an empty vector on null or any parse failure — tags are decorative,
/// never load-bearing.
pub fn parse_tags(stored: Option<&str>) -> Vec<String> {
    stored
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

pub fn now_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
