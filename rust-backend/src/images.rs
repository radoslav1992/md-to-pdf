//! User-uploaded images. Stored as blobs in SQLite (see migration 0005).
//!
//! Images are referenced from documents via `/api/images/{id}`. For PDF
//! rendering, those URLs are rewritten to inline `data:` URIs in
//! `convert::run` so Chromium (which loads the page from `file://`) can
//! resolve them.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::db::{now_seconds, User};
use crate::error::ConvertError;

/// 10 MiB upload cap per image. Larger than any reasonable inline image; we
/// don't want to host arbitrary file storage.
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
/// Total bytes across all of a user's images. Soft cap to keep the SQLite
/// file from ballooning out of the named volume.
pub const MAX_TOTAL_BYTES_PER_USER: i64 = 256 * 1024 * 1024;

const ALLOWED_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/jpg",
    "image/gif",
    "image/webp",
    "image/svg+xml",
];

#[derive(Debug, Deserialize)]
pub struct UploadImage {
    pub filename: String,
    pub content_type: String,
    /// Base64-encoded image bytes (no `data:` prefix).
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ImageMeta {
    pub id: i64,
    pub user_id: i64,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct ImageUploadResult {
    pub id: i64,
    pub url: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub created_at: i64,
}

pub async fn upload(
    pool: &SqlitePool,
    user: &User,
    payload: &UploadImage,
) -> Result<ImageUploadResult, ConvertError> {
    let content_type = payload.content_type.trim().to_ascii_lowercase();
    if !ALLOWED_TYPES.contains(&content_type.as_str()) {
        return Err(ConvertError::BadRequest(format!(
            "unsupported image content type: {content_type}"
        )));
    }

    let filename = sanitize_filename(payload.filename.trim());
    if filename.is_empty() {
        return Err(ConvertError::BadRequest("filename is required".into()));
    }

    let bytes = STANDARD
        .decode(payload.data_base64.trim())
        .map_err(|e| ConvertError::BadRequest(format!("invalid base64: {e}")))?;
    if bytes.is_empty() {
        return Err(ConvertError::BadRequest("image is empty".into()));
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ConvertError::PayloadTooLarge(bytes.len(), MAX_IMAGE_BYTES));
    }

    // Enforce a per-user storage budget so a single account can't fill the
    // disk. Counted across all images.
    let total: (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(size_bytes), 0) FROM images WHERE user_id = ?1")
            .bind(user.id)
            .fetch_one(pool)
            .await?;
    if total.0 + bytes.len() as i64 > MAX_TOTAL_BYTES_PER_USER {
        return Err(ConvertError::BadRequest(format!(
            "per-user image storage limit reached ({} MiB)",
            MAX_TOTAL_BYTES_PER_USER / (1024 * 1024)
        )));
    }

    let sha = {
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        hex::encode(hasher.finalize())
    };

    // Deduplicate per user — uploading the same file twice returns the
    // existing record so editors that drag the same image in repeatedly
    // don't accumulate duplicates.
    if let Some(existing) = sqlx::query_as::<_, ImageMeta>(
        "SELECT id, user_id, filename, content_type, size_bytes, sha256, created_at \
         FROM images WHERE user_id = ?1 AND sha256 = ?2 LIMIT 1",
    )
    .bind(user.id)
    .bind(&sha)
    .fetch_optional(pool)
    .await?
    {
        return Ok(ImageUploadResult {
            id: existing.id,
            url: format!("/api/images/{}", existing.id),
            filename: existing.filename,
            content_type: existing.content_type,
            size_bytes: existing.size_bytes,
            created_at: existing.created_at,
        });
    }

    let now = now_seconds();
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO images (user_id, filename, content_type, size_bytes, sha256, data, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
    )
    .bind(user.id)
    .bind(&filename)
    .bind(&content_type)
    .bind(bytes.len() as i64)
    .bind(&sha)
    .bind(&bytes)
    .bind(now)
    .fetch_one(pool)
    .await?;

    Ok(ImageUploadResult {
        id: row.0,
        url: format!("/api/images/{}", row.0),
        filename,
        content_type,
        size_bytes: bytes.len() as i64,
        created_at: now,
    })
}

pub async fn fetch(
    pool: &SqlitePool,
    user: &User,
    id: i64,
) -> Result<(ImageMeta, Vec<u8>), ConvertError> {
    let row: Option<(i64, i64, String, String, i64, String, Vec<u8>, i64)> = sqlx::query_as(
        "SELECT id, user_id, filename, content_type, size_bytes, sha256, data, created_at \
         FROM images WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(pool)
    .await?;
    let row = row.ok_or(ConvertError::NotFound)?;
    let meta = ImageMeta {
        id: row.0,
        user_id: row.1,
        filename: row.2,
        content_type: row.3,
        size_bytes: row.4,
        sha256: row.5,
        created_at: row.7,
    };
    Ok((meta, row.6))
}

pub async fn list(pool: &SqlitePool, user: &User) -> Result<Vec<ImageMeta>, ConvertError> {
    let items = sqlx::query_as::<_, ImageMeta>(
        "SELECT id, user_id, filename, content_type, size_bytes, sha256, created_at \
         FROM images WHERE user_id = ?1 ORDER BY created_at DESC",
    )
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

pub async fn delete(pool: &SqlitePool, user: &User, id: i64) -> Result<(), ConvertError> {
    let res = sqlx::query("DELETE FROM images WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(user.id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ConvertError::NotFound);
    }
    Ok(())
}

/// Encode image bytes as a `data:` URI suitable for inlining in HTML.
pub fn to_data_uri(content_type: &str, bytes: &[u8]) -> String {
    format!("data:{};base64,{}", content_type, STANDARD.encode(bytes))
}

fn sanitize_filename(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(*c, '.' | '-' | '_'))
        .take(120)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_unsafe_chars() {
        // Slashes, spaces, and parentheses are dropped; dots, dashes, and
        // underscores are preserved so common filenames survive intact.
        assert_eq!(sanitize_filename("../../etc/passwd"), "....etcpasswd");
        assert_eq!(sanitize_filename("photo (1).png"), "photo1.png");
        assert_eq!(sanitize_filename("my-image_v2.png"), "my-image_v2.png");
    }

    #[test]
    fn data_uri_format() {
        let uri = to_data_uri("image/png", &[0xff, 0xd8]);
        assert!(uri.starts_with("data:image/png;base64,"));
    }
}
