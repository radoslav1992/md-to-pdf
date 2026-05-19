//! Document version history. Every `documents::update` snapshots the
//! pre-update row here so the user can list past versions and restore.
//! The oldest rows beyond `MAX_VERSIONS` per document are pruned.

use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

use crate::db::{now_seconds, Document, User};
use crate::error::ConvertError;

const MAX_VERSIONS: i64 = 50;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct DocumentVersion {
    pub id: i64,
    pub document_id: i64,
    pub version: i64,
    pub title: String,
    pub input_type: String,
    pub output_type: String,
    pub content: String,
    pub rendered_html: Option<String>,
    pub theme: Option<String>,
    pub custom_css: Option<String>,
    pub pdf_options: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentVersionSummary {
    pub id: i64,
    pub document_id: i64,
    pub version: i64,
    pub title: String,
    pub content_bytes: i64,
    pub created_at: i64,
}

impl From<&DocumentVersion> for DocumentVersionSummary {
    fn from(v: &DocumentVersion) -> Self {
        Self {
            id: v.id,
            document_id: v.document_id,
            version: v.version,
            title: v.title.clone(),
            content_bytes: v.content.len() as i64,
            created_at: v.created_at,
        }
    }
}

/// Snapshot the given document into the versions table. Called from
/// `documents::update` BEFORE the update is applied.
pub async fn snapshot(pool: &SqlitePool, doc: &Document) -> Result<(), ConvertError> {
    let next_version: (i64,) = sqlx::query_as(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM document_versions WHERE document_id = ?1",
    )
    .bind(doc.id)
    .fetch_one(pool)
    .await?;

    sqlx::query(
        "INSERT INTO document_versions \
         (document_id, version, title, input_type, output_type, content, rendered_html, \
          theme, custom_css, pdf_options, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )
    .bind(doc.id)
    .bind(next_version.0)
    .bind(&doc.title)
    .bind(&doc.input_type)
    .bind(&doc.output_type)
    .bind(&doc.content)
    .bind(doc.rendered_html.as_deref())
    .bind(doc.theme.as_deref())
    .bind(doc.custom_css.as_deref())
    .bind(doc.pdf_options.as_deref())
    .bind(now_seconds())
    .execute(pool)
    .await?;

    // Prune anything beyond MAX_VERSIONS so a doc edited daily doesn't grow
    // unboundedly. We keep the highest version numbers.
    sqlx::query(
        "DELETE FROM document_versions WHERE document_id = ?1 AND id NOT IN \
         (SELECT id FROM document_versions WHERE document_id = ?1 \
          ORDER BY version DESC LIMIT ?2)",
    )
    .bind(doc.id)
    .bind(MAX_VERSIONS)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list(
    pool: &SqlitePool,
    user: &User,
    document_id: i64,
) -> Result<Vec<DocumentVersionSummary>, ConvertError> {
    ensure_owns_document(pool, user, document_id).await?;
    let rows: Vec<DocumentVersion> = sqlx::query_as(
        "SELECT id, document_id, version, title, input_type, output_type, content, \
                rendered_html, theme, custom_css, pdf_options, created_at \
         FROM document_versions WHERE document_id = ?1 ORDER BY version DESC",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(DocumentVersionSummary::from).collect())
}

pub async fn get(
    pool: &SqlitePool,
    user: &User,
    document_id: i64,
    version_id: i64,
) -> Result<DocumentVersion, ConvertError> {
    ensure_owns_document(pool, user, document_id).await?;
    let row: Option<DocumentVersion> = sqlx::query_as(
        "SELECT id, document_id, version, title, input_type, output_type, content, \
                rendered_html, theme, custom_css, pdf_options, created_at \
         FROM document_versions WHERE id = ?1 AND document_id = ?2",
    )
    .bind(version_id)
    .bind(document_id)
    .fetch_optional(pool)
    .await?;
    row.ok_or(ConvertError::NotFound)
}

async fn ensure_owns_document(
    pool: &SqlitePool,
    user: &User,
    document_id: i64,
) -> Result<(), ConvertError> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM documents WHERE id = ?1 AND user_id = ?2")
            .bind(document_id)
            .bind(user.id)
            .fetch_optional(pool)
            .await?;
    if row.is_none() {
        return Err(ConvertError::NotFound);
    }
    Ok(())
}
