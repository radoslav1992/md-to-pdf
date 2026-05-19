//! Bearer-token authentication for programmatic access.
//!
//! Keys are 32 random bytes hex-encoded with a `udc_` prefix; only their
//! SHA-256 hash is persisted. The plaintext is returned to the user
//! exactly once at creation time. Lookups are O(1) via the indexed
//! `key_hash` column.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};

use crate::crypto;
use crate::db::{now_seconds, User};
use crate::error::ConvertError;

const KEY_PREFIX: &str = "udc_";
const MAX_NAME: usize = 80;
const MAX_PER_USER: i64 = 25;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ApiKey {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub prefix: String,
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    pub key_hash: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateKey {
    pub name: String,
}

/// Hex-SHA-256 of the plaintext key. Pure function so tests don't need a DB.
pub fn hash(plaintext: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plaintext.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn parse_bearer(header_value: &str) -> Option<&str> {
    let trimmed = header_value.trim();
    let token = trimmed.strip_prefix("Bearer ")?.trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

pub async fn lookup(pool: &SqlitePool, plaintext: &str) -> Result<Option<(ApiKey, User)>, ConvertError> {
    let h = hash(plaintext);
    let key: Option<ApiKey> = sqlx::query_as(
        "SELECT id, user_id, name, prefix, key_hash, created_at, last_used_at \
         FROM api_keys WHERE key_hash = ?1",
    )
    .bind(&h)
    .fetch_optional(pool)
    .await?;
    let Some(key) = key else { return Ok(None) };
    let user: Option<User> = sqlx::query_as(
        "SELECT id, email, password_hash, password_salt, role, created_at \
         FROM users WHERE id = ?1",
    )
    .bind(key.user_id)
    .fetch_optional(pool)
    .await?;
    Ok(user.map(|u| (key, u)))
}

pub async fn touch(pool: &SqlitePool, key_id: i64) {
    let _ = sqlx::query("UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2")
        .bind(now_seconds())
        .bind(key_id)
        .execute(pool)
        .await;
}

pub async fn list(pool: &SqlitePool, user: &User) -> Result<Vec<ApiKey>, ConvertError> {
    Ok(sqlx::query_as(
        "SELECT id, user_id, name, prefix, key_hash, created_at, last_used_at \
         FROM api_keys WHERE user_id = ?1 ORDER BY created_at DESC",
    )
    .bind(user.id)
    .fetch_all(pool)
    .await?)
}

/// Create a new key. Returns the full plaintext key — only chance to see it.
pub async fn create(
    pool: &SqlitePool,
    user: &User,
    payload: &CreateKey,
) -> Result<(ApiKey, String), ConvertError> {
    let name = payload.name.trim();
    if name.is_empty() || name.len() > MAX_NAME {
        return Err(ConvertError::BadRequest(
            "key name must be 1..=80 characters".into(),
        ));
    }
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM api_keys WHERE user_id = ?1")
            .bind(user.id)
            .fetch_one(pool)
            .await?;
    if count.0 >= MAX_PER_USER {
        return Err(ConvertError::Conflict(format!(
            "API key limit reached ({MAX_PER_USER} per user)"
        )));
    }

    let plaintext = format!("{KEY_PREFIX}{}", crypto::random_hex(32));
    let prefix: String = plaintext.chars().take(12).collect();
    let key_hash = hash(&plaintext);
    let now = now_seconds();

    let id: (i64,) = sqlx::query_as(
        "INSERT INTO api_keys (user_id, name, prefix, key_hash, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
    )
    .bind(user.id)
    .bind(name)
    .bind(&prefix)
    .bind(&key_hash)
    .bind(now)
    .fetch_one(pool)
    .await?;

    let key = ApiKey {
        id: id.0,
        user_id: user.id,
        name: name.to_string(),
        prefix,
        key_hash,
        created_at: now,
        last_used_at: None,
    };
    Ok((key, plaintext))
}

pub async fn revoke(pool: &SqlitePool, user: &User, id: i64) -> Result<(), ConvertError> {
    let res = sqlx::query("DELETE FROM api_keys WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(user.id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ConvertError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        assert_eq!(hash("hello"), hash("hello"));
        assert_ne!(hash("hello"), hash("Hello"));
        assert_eq!(hash("").len(), 64);
    }

    #[test]
    fn parse_bearer_strips_prefix() {
        assert_eq!(parse_bearer("Bearer abc"), Some("abc"));
        assert_eq!(parse_bearer("  Bearer  abc  "), Some("abc"));
        assert_eq!(parse_bearer("bearer abc"), None);
        assert_eq!(parse_bearer("Bearer "), None);
        assert_eq!(parse_bearer(""), None);
    }
}
