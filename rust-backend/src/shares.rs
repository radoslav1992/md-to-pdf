//! Public share links. A premium user generates a token; anyone with the
//! URL can view the document in HTML or PDF without an account. Optional
//! password gate and expiration; revocable. The plaintext token is only
//! returned once at creation; the table stores a SHA-256 hash.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};

use crate::crypto;
use crate::db::{now_seconds, User};
use crate::error::ConvertError;

const TOKEN_PREFIX: &str = "udcs_";
const MAX_PER_DOCUMENT: i64 = 20;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ShareLink {
    pub id: i64,
    pub document_id: i64,
    pub user_id: i64,
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    pub token_hash: String,
    pub prefix: String,
    pub format: String,
    pub expires_at: Option<i64>,
    #[serde(skip_serializing)]
    pub password_hash: Option<String>,
    #[serde(skip_serializing)]
    pub password_salt: Option<String>,
    pub view_count: i64,
    pub created_at: i64,
}

impl ShareLink {
    pub fn requires_password(&self) -> bool {
        self.password_hash.is_some()
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateShare {
    /// One of "html" or "pdf". Pdf shares only work for documents whose
    /// stored output_type is "pdf".
    #[serde(default = "default_format")]
    pub format: String,
    /// Seconds from now until the share expires. Omit for no expiry.
    #[serde(default)]
    pub expires_in_seconds: Option<i64>,
    #[serde(default)]
    pub password: Option<String>,
}

fn default_format() -> String {
    "html".to_string()
}

pub fn hash_token(plaintext: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plaintext.as_bytes());
    hex::encode(hasher.finalize())
}

pub async fn create(
    pool: &SqlitePool,
    user: &User,
    document_id: i64,
    payload: &CreateShare,
) -> Result<(ShareLink, String), ConvertError> {
    if !matches!(payload.format.as_str(), "html" | "pdf") {
        return Err(ConvertError::BadRequest(
            "share format must be html or pdf".into(),
        ));
    }
    // Sanity-check the document exists and is owned by the user.
    let doc_row: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM documents WHERE id = ?1 AND user_id = ?2")
            .bind(document_id)
            .bind(user.id)
            .fetch_optional(pool)
            .await?;
    if doc_row.is_none() {
        return Err(ConvertError::NotFound);
    }

    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM share_links WHERE document_id = ?1")
            .bind(document_id)
            .fetch_one(pool)
            .await?;
    if count.0 >= MAX_PER_DOCUMENT {
        return Err(ConvertError::Conflict(format!(
            "share-link limit reached ({MAX_PER_DOCUMENT} per document)"
        )));
    }

    let plaintext = format!("{TOKEN_PREFIX}{}", crypto::random_hex(24));
    let prefix: String = plaintext.chars().take(13).collect();
    let token_hash = hash_token(&plaintext);
    let (pwd_hash, pwd_salt) = match payload.password.as_deref() {
        Some(p) if !p.is_empty() => {
            if p.len() < 4 {
                return Err(ConvertError::BadRequest(
                    "share password must be at least 4 characters".into(),
                ));
            }
            let (h, s) = crypto::hash_password(p)?;
            (Some(h), Some(s))
        }
        _ => (None, None),
    };
    let now = now_seconds();
    let expires_at = payload
        .expires_in_seconds
        .filter(|n| *n > 0)
        .map(|n| now + n);

    let id: (i64,) = sqlx::query_as(
        "INSERT INTO share_links \
         (document_id, user_id, token_hash, prefix, format, expires_at, password_hash, \
          password_salt, view_count, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9) RETURNING id",
    )
    .bind(document_id)
    .bind(user.id)
    .bind(&token_hash)
    .bind(&prefix)
    .bind(&payload.format)
    .bind(expires_at)
    .bind(pwd_hash.as_deref())
    .bind(pwd_salt.as_deref())
    .bind(now)
    .fetch_one(pool)
    .await?;

    let link = ShareLink {
        id: id.0,
        document_id,
        user_id: user.id,
        token_hash,
        prefix,
        format: payload.format.clone(),
        expires_at,
        password_hash: pwd_hash,
        password_salt: pwd_salt,
        view_count: 0,
        created_at: now,
    };
    Ok((link, plaintext))
}

pub async fn list_for_document(
    pool: &SqlitePool,
    user: &User,
    document_id: i64,
) -> Result<Vec<ShareLink>, ConvertError> {
    let rows: Vec<ShareLink> = sqlx::query_as(
        "SELECT id, document_id, user_id, token_hash, prefix, format, expires_at, \
                password_hash, password_salt, view_count, created_at \
         FROM share_links WHERE document_id = ?1 AND user_id = ?2 ORDER BY created_at DESC",
    )
    .bind(document_id)
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn revoke(pool: &SqlitePool, user: &User, id: i64) -> Result<(), ConvertError> {
    let res = sqlx::query("DELETE FROM share_links WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(user.id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ConvertError::NotFound);
    }
    Ok(())
}

pub async fn lookup(pool: &SqlitePool, plaintext_token: &str) -> Result<ShareLink, ConvertError> {
    let h = hash_token(plaintext_token);
    let row: Option<ShareLink> = sqlx::query_as(
        "SELECT id, document_id, user_id, token_hash, prefix, format, expires_at, \
                password_hash, password_salt, view_count, created_at \
         FROM share_links WHERE token_hash = ?1",
    )
    .bind(&h)
    .fetch_optional(pool)
    .await?;
    let link = row.ok_or(ConvertError::NotFound)?;
    if let Some(exp) = link.expires_at {
        if exp < now_seconds() {
            return Err(ConvertError::NotFound);
        }
    }
    Ok(link)
}

pub fn verify_password(link: &ShareLink, password: Option<&str>) -> Result<(), ConvertError> {
    let Some(hash) = link.password_hash.as_deref() else {
        return Ok(());
    };
    let salt = link.password_salt.as_deref().unwrap_or("");
    let password = password.ok_or_else(|| ConvertError::Unauthorized)?;
    if !crypto::verify_password(password, salt, hash) {
        return Err(ConvertError::Unauthorized);
    }
    Ok(())
}

pub async fn bump_view_count(pool: &SqlitePool, id: i64) {
    let _ = sqlx::query("UPDATE share_links SET view_count = view_count + 1 WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await;
}
