use serde::Deserialize;

use crate::db::{now_seconds, Document, DocumentSummary, User};
use crate::doc_crypto;
use crate::document_versions;
use crate::error::ConvertError;
use sqlx::SqlitePool;

const MAX_TITLE: usize = 200;
const MAX_CONTENT: usize = 16 * 1024 * 1024;
/// Filtering on tags happens in SQLite via LIKE on the JSON column, so we
/// keep each tag short and printable.
const MAX_TAG_LEN: usize = 32;
const MAX_TAGS_PER_DOC: usize = 16;

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
    /// Folder path. Free-form string, normalized to forward slashes. Empty
    /// or unset means "no folder" (top-level).
    #[serde(default)]
    pub folder: Option<String>,
    /// Tags. Capped at 16 entries of 32 chars each, lowercased.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

const SELECT_COLUMNS: &str = "id, user_id, title, input_type, output_type, content, rendered_html, \
     theme, custom_css, pdf_options, is_encrypted, encryption_salt, folder, tags, created_at, updated_at";

/// Optional filter set for the documents list endpoint. All fields are
/// independent — folder + tag + query can be combined.
#[derive(Debug, Default, Deserialize)]
pub struct ListFilter {
    /// Full-text query against the documents_fts virtual table. Empty or
    /// whitespace-only strings are ignored.
    #[serde(default)]
    pub q: Option<String>,
    /// Exact folder match. Pass an empty string to look up top-level
    /// (folder IS NULL OR folder = '').
    #[serde(default)]
    pub folder: Option<String>,
    /// Single tag. Matches if the document's JSON tag array contains it.
    #[serde(default)]
    pub tag: Option<String>,
}

pub async fn list(
    pool: &SqlitePool,
    user: &User,
    filter: &ListFilter,
) -> Result<Vec<DocumentSummary>, ConvertError> {
    let q = filter
        .q
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let folder_filter = filter.folder.as_deref().map(str::trim);
    let tag = filter
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase());

    // FTS path: when there's a query, join the FTS index against
    // documents so we still get full row data. The user_id column in
    // the FTS table is UNINDEXED but lets us filter without a join back
    // first.
    let docs: Vec<Document> = if let Some(query) = q {
        let fts_query = build_fts_query(&query);
        let mut sql = format!(
            "SELECT {SELECT_COLUMNS} FROM documents d \
             JOIN documents_fts fts ON fts.rowid = d.id \
             WHERE d.user_id = ?1 AND fts.user_id = ?1 AND documents_fts MATCH ?2"
        );
        let mut params_idx = 3;
        if let Some(f) = folder_filter {
            if f.is_empty() {
                sql.push_str(" AND (d.folder IS NULL OR d.folder = '')");
            } else {
                sql.push_str(&format!(" AND d.folder = ?{params_idx}"));
                params_idx += 1;
            }
        }
        if tag.is_some() {
            sql.push_str(&format!(" AND d.tags LIKE ?{params_idx}"));
        }
        sql.push_str(" ORDER BY rank LIMIT 200");

        let mut q = sqlx::query_as::<_, Document>(&sql)
            .bind(user.id)
            .bind(fts_query);
        if let Some(f) = folder_filter {
            if !f.is_empty() {
                q = q.bind(f.to_string());
            }
        }
        if let Some(t) = &tag {
            q = q.bind(format!("%\"{}%", t));
        }
        q.fetch_all(pool).await?
    } else {
        let mut sql = format!(
            "SELECT {SELECT_COLUMNS} FROM documents WHERE user_id = ?1"
        );
        let mut params_idx = 2;
        if let Some(f) = folder_filter {
            if f.is_empty() {
                sql.push_str(" AND (folder IS NULL OR folder = '')");
            } else {
                sql.push_str(&format!(" AND folder = ?{params_idx}"));
                params_idx += 1;
            }
        }
        if tag.is_some() {
            sql.push_str(&format!(" AND tags LIKE ?{params_idx}"));
        }
        sql.push_str(" ORDER BY updated_at DESC LIMIT 500");

        let mut q = sqlx::query_as::<_, Document>(&sql).bind(user.id);
        if let Some(f) = folder_filter {
            if !f.is_empty() {
                q = q.bind(f.to_string());
            }
        }
        if let Some(t) = &tag {
            q = q.bind(format!("%\"{}%", t));
        }
        q.fetch_all(pool).await?
    };

    Ok(docs.iter().map(DocumentSummary::from).collect())
}

/// Distinct folders the user has documents in. Used to populate the
/// sidebar tree on the dashboard.
pub async fn list_folders(pool: &SqlitePool, user: &User) -> Result<Vec<String>, ConvertError> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT folder FROM documents \
         WHERE user_id = ?1 AND folder IS NOT NULL AND folder <> '' \
         ORDER BY folder",
    )
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(s,)| s).collect())
}

/// Distinct tags the user has applied across their documents.
pub async fn list_tags(pool: &SqlitePool, user: &User) -> Result<Vec<String>, ConvertError> {
    let rows: Vec<(Option<String>,)> = sqlx::query_as(
        "SELECT tags FROM documents WHERE user_id = ?1 AND tags IS NOT NULL AND tags <> '[]'",
    )
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    let mut all: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (raw,) in rows {
        for t in crate::db::parse_tags(raw.as_deref()) {
            all.insert(t);
        }
    }
    Ok(all.into_iter().collect())
}

/// Translate a raw user query into a FTS5 MATCH expression. We:
///   1. lower-case everything,
///   2. split on whitespace,
///   3. strip anything that isn't alphanumeric, dash, underscore, or dot
///      (so users can't smuggle in FTS5 operators like NEAR/AND/OR),
///   4. attach a `*` so prefix matches work mid-word.
///
/// The result is safe to pass directly to `documents_fts MATCH ?`.
fn build_fts_query(input: &str) -> String {
    let parts: Vec<String> = input
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric() || matches!(*c, '-' | '_' | '.'))
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w))
        .collect();
    if parts.is_empty() {
        // FTS5 requires non-empty MATCH expressions; fall back to a no-op
        // that won't match anything rather than erroring out.
        "\"\\u0000\"".to_string()
    } else {
        parts.join(" ")
    }
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
    let folder = normalize_folder(payload.folder.as_deref());
    let tags_json = normalize_tags(payload.tags.as_deref())?;
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO documents \
         (user_id, title, input_type, output_type, content, rendered_html, \
          theme, custom_css, pdf_options, is_encrypted, encryption_salt, folder, tags, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14) RETURNING id",
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
    .bind(folder.as_deref())
    .bind(tags_json.as_deref())
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
    let folder = normalize_folder(payload.folder.as_deref());
    let tags_json = normalize_tags(payload.tags.as_deref())?;

    let result = sqlx::query(
        "UPDATE documents SET title = ?1, input_type = ?2, output_type = ?3, content = ?4, \
         rendered_html = ?5, theme = ?6, custom_css = ?7, pdf_options = ?8, \
         is_encrypted = ?9, encryption_salt = ?10, folder = ?11, tags = ?12, updated_at = ?13 \
         WHERE id = ?14 AND user_id = ?15",
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
    .bind(folder.as_deref())
    .bind(tags_json.as_deref())
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

fn normalize_folder(input: Option<&str>) -> Option<String> {
    let s = input?.trim();
    if s.is_empty() {
        return None;
    }
    // Collapse backslashes and double slashes; cap depth for sanity.
    let normalized: String = s
        .replace('\\', "/")
        .split('/')
        .map(|seg| seg.trim())
        .filter(|seg| !seg.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("/");
    if normalized.is_empty() || normalized.len() > 240 {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_tags(input: Option<&[String]>) -> Result<Option<String>, ConvertError> {
    let Some(raw) = input else {
        return Ok(None);
    };
    let mut seen = std::collections::BTreeSet::<String>::new();
    for tag in raw.iter().take(MAX_TAGS_PER_DOC * 2) {
        let cleaned: String = tag
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(*c, '-' | '_'))
            .take(MAX_TAG_LEN)
            .collect();
        if !cleaned.is_empty() {
            seen.insert(cleaned);
        }
        if seen.len() >= MAX_TAGS_PER_DOC {
            break;
        }
    }
    if seen.is_empty() {
        return Ok(None);
    }
    let list: Vec<String> = seen.into_iter().collect();
    let json = serde_json::to_string(&list).map_err(|e| ConvertError::Internal(e.to_string()))?;
    Ok(Some(json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_normalisation_trims_segments() {
        assert_eq!(normalize_folder(Some("  ")), None);
        assert_eq!(normalize_folder(Some("Work")), Some("Work".into()));
        assert_eq!(
            normalize_folder(Some("Work / Projects / 2026")),
            Some("Work/Projects/2026".into())
        );
        assert_eq!(
            normalize_folder(Some("\\Work\\Drafts\\")),
            Some("Work/Drafts".into())
        );
    }

    #[test]
    fn tag_normalisation_caps_and_dedups() {
        let raw: Vec<String> = vec!["Draft".into(), "draft".into(), "WIP!".into(), "".into()];
        let out = normalize_tags(Some(&raw)).unwrap().unwrap();
        // BTreeSet means alphabetical order.
        assert_eq!(out, "[\"draft\",\"wip\"]");
    }

    #[test]
    fn fts_query_strips_operators() {
        // Operators like NEAR/AND would otherwise cause MATCH to error out.
        let q = build_fts_query("hello AND world OR foo*");
        assert!(q.contains("\"hello\"*"));
        assert!(q.contains("\"and\"*"));
        assert!(q.contains("\"or\"*"));
        // FTS5 syntax characters like `*` and quotes are stripped from
        // the user-supplied portion before being re-attached.
        assert!(q.contains("\"foo\"*"));
    }

    #[test]
    fn fts_query_falls_back_for_empty_input() {
        let q = build_fts_query("    ");
        assert!(!q.is_empty());
    }
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
