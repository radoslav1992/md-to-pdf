use serde::Deserialize;

use crate::db::{now_seconds, Document, DocumentSummary, User};
use crate::doc_crypto;
use crate::document_versions;
use crate::error::ConvertError;
use sqlx::SqlitePool;

const MAX_TITLE: usize = 200;
const MAX_CONTENT: usize = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct SaveDocument {
    pub title: String,
    #[serde(rename = "type")]
    pub input_type: String,
    pub output: String,
    pub content: String,
    pub rendered_html: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub custom_css: Option<String>,
    #[serde(default)]
    pub pdf_options: Option<serde_json::Value>,
    /// When set, the document is encrypted at rest under AES-256-GCM with a
    /// key derived from this password. The plaintext password is discarded
    /// after key derivation and never persisted.
    #[serde(default)]
    pub encrypt_password: Option<String>,
}

const SELECT_COLUMNS: &str = "id, user_id, title, input_type, output_type, content, rendered_html, \
     theme, custom_css, pdf_options, is_encrypted, encryption_salt, created_at, updated_at";

pub async fn list(pool: &SqlitePool, user: &User) -> Result<Vec<DocumentSummary>, ConvertError> {
    let docs: Vec<Document> = sqlx::query_as(&format!(
        "SELECT {SELECT_COLUMNS} \
         FROM documents WHERE user_id = ?1 ORDER BY updated_at DESC"
    ))
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    Ok(docs.iter().map(DocumentSummary::from).collect())
}

pub async fn get(pool: &SqlitePool, user: &User, id: i64) -> Result<Document, ConvertError> {
    let doc: Option<Document> = sqlx::query_as(&format!(
        "SELECT {SELECT_COLUMNS} \
         FROM documents WHERE id = ?1 AND user_id = ?2"
    ))
    .bind(id)
    .bind(user.id)
    .fetch_optional(pool)
    .await?;
    doc.ok_or(ConvertError::NotFound)
}

pub async fn save(
    pool: &SqlitePool,
    user: &User,
    payload: &SaveDocument,
) -> Result<Document, ConvertError> {
    let title = payload.title.trim();
    if title.is_empty() || title.len() > MAX_TITLE {
        return Err(ConvertError::BadRequest(
            "title must be 1..=200 characters".into(),
        ));
    }
    if payload.content.len() > MAX_CONTENT {
        return Err(ConvertError::PayloadTooLarge(
            payload.content.len(),
            MAX_CONTENT,
        ));
    }

    let (content_to_store, is_encrypted, encryption_salt) = match payload.encrypt_password.as_deref()
    {
        Some(pw) if !pw.is_empty() => {
            if !user.is_premium() {
                return Err(ConvertError::PremiumRequired);
            }
            let enc = doc_crypto::encrypt(pw, &payload.content)?;
            (enc.blob, 1i64, Some(enc.salt_hex))
        }
        _ => (payload.content.clone(), 0i64, None),
    };

    let now = now_seconds();
    let rendered = payload.rendered_html.clone().unwrap_or_default();
    let pdf_options_json = payload
        .pdf_options
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO documents \
         (user_id, title, input_type, output_type, content, rendered_html, \
          theme, custom_css, pdf_options, is_encrypted, encryption_salt, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12) RETURNING id",
    )
    .bind(user.id)
    .bind(title)
    .bind(&payload.input_type)
    .bind(&payload.output)
    .bind(&content_to_store)
    .bind(&rendered)
    .bind(payload.theme.as_deref())
    .bind(payload.custom_css.as_deref())
    .bind(pdf_options_json.as_deref())
    .bind(is_encrypted)
    .bind(encryption_salt.as_deref())
    .bind(now)
    .fetch_one(pool)
    .await?;
    get(pool, user, row.0).await
}

pub async fn update(
    pool: &SqlitePool,
    user: &User,
    id: i64,
    payload: &SaveDocument,
) -> Result<Document, ConvertError> {
    let title = payload.title.trim();
    if title.is_empty() || title.len() > MAX_TITLE {
        return Err(ConvertError::BadRequest(
            "title must be 1..=200 characters".into(),
        ));
    }
    if payload.content.len() > MAX_CONTENT {
        return Err(ConvertError::PayloadTooLarge(
            payload.content.len(),
            MAX_CONTENT,
        ));
    }

    // Snapshot the pre-update state so the user can restore later.
    let existing = get(pool, user, id).await?;
    document_versions::snapshot(pool, &existing).await?;

    let (content_to_store, is_encrypted, encryption_salt) = match payload.encrypt_password.as_deref()
    {
        Some(pw) if !pw.is_empty() => {
            if !user.is_premium() {
                return Err(ConvertError::PremiumRequired);
            }
            let enc = doc_crypto::encrypt(pw, &payload.content)?;
            (enc.blob, 1i64, Some(enc.salt_hex))
        }
        _ => (payload.content.clone(), 0i64, None),
    };

    let now = now_seconds();
    let rendered = payload.rendered_html.clone().unwrap_or_default();
    let pdf_options_json = payload
        .pdf_options
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());

    let result = sqlx::query(
        "UPDATE documents SET title = ?1, input_type = ?2, output_type = ?3, content = ?4, \
         rendered_html = ?5, theme = ?6, custom_css = ?7, pdf_options = ?8, \
         is_encrypted = ?9, encryption_salt = ?10, updated_at = ?11 \
         WHERE id = ?12 AND user_id = ?13",
    )
    .bind(title)
    .bind(&payload.input_type)
    .bind(&payload.output)
    .bind(&content_to_store)
    .bind(&rendered)
    .bind(payload.theme.as_deref())
    .bind(payload.custom_css.as_deref())
    .bind(pdf_options_json.as_deref())
    .bind(is_encrypted)
    .bind(encryption_salt.as_deref())
    .bind(now)
    .bind(id)
    .bind(user.id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(ConvertError::NotFound);
    }

    get(pool, user, id).await
}

pub async fn delete(pool: &SqlitePool, user: &User, id: i64) -> Result<(), ConvertError> {
    let result = sqlx::query("DELETE FROM documents WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(user.id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ConvertError::NotFound);
    }
    Ok(())
}

/// Decrypt a stored encrypted document. Errors if the doc isn't encrypted
/// or the password is wrong. Returns the plaintext content.
pub async fn decrypt_content(
    pool: &SqlitePool,
    user: &User,
    id: i64,
    password: &str,
) -> Result<String, ConvertError> {
    let doc = get(pool, user, id).await?;
    if doc.is_encrypted == 0 {
        return Err(ConvertError::BadRequest(
            "document is not encrypted".into(),
        ));
    }
    let salt = doc.encryption_salt.as_deref().ok_or_else(|| {
        ConvertError::Internal("encrypted document has no salt".into())
    })?;
    doc_crypto::decrypt(password, salt, &doc.content)
}

/// Apply a stored prior version to the document. The current state is
/// snapshotted first so the restore itself is reversible.
pub async fn restore_version(
    pool: &SqlitePool,
    user: &User,
    document_id: i64,
    version_id: i64,
) -> Result<Document, ConvertError> {
    let current = get(pool, user, document_id).await?;
    let version = document_versions::get(pool, user, document_id, version_id).await?;
    document_versions::snapshot(pool, &current).await?;

    let now = now_seconds();
    sqlx::query(
        "UPDATE documents SET title = ?1, input_type = ?2, output_type = ?3, content = ?4, \
         rendered_html = ?5, theme = ?6, custom_css = ?7, pdf_options = ?8, updated_at = ?9 \
         WHERE id = ?10 AND user_id = ?11",
    )
    .bind(&version.title)
    .bind(&version.input_type)
    .bind(&version.output_type)
    .bind(&version.content)
    .bind(version.rendered_html.as_deref())
    .bind(version.theme.as_deref())
    .bind(version.custom_css.as_deref())
    .bind(version.pdf_options.as_deref())
    .bind(now)
    .bind(document_id)
    .bind(user.id)
    .execute(pool)
    .await?;
    get(pool, user, document_id).await
}
