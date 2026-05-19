use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub admin_emails: Vec<String>,
    pub cookie_secure: bool,
    pub chromium_bin: String,
    pub pdf_semaphore: Arc<Semaphore>,
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
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentSummary {
    pub id: i64,
    pub title: String,
    pub input_type: String,
    pub output_type: String,
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
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

pub fn now_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
