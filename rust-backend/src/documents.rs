use serde::Deserialize;

use crate::db::{now_seconds, Document, DocumentSummary, User};
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
}

pub async fn list(pool: &SqlitePool, user: &User) -> Result<Vec<DocumentSummary>, ConvertError> {
    let docs: Vec<Document> = sqlx::query_as(
        "SELECT id, user_id, title, input_type, output_type, content, rendered_html, \
                theme, custom_css, pdf_options, created_at, updated_at \
         FROM documents WHERE user_id = ?1 ORDER BY updated_at DESC",
    )
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    Ok(docs.iter().map(DocumentSummary::from).collect())
}

pub async fn get(pool: &SqlitePool, user: &User, id: i64) -> Result<Document, ConvertError> {
    let doc: Option<Document> = sqlx::query_as(
        "SELECT id, user_id, title, input_type, output_type, content, rendered_html, \
                theme, custom_css, pdf_options, created_at, updated_at \
         FROM documents WHERE id = ?1 AND user_id = ?2",
    )
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

    let now = now_seconds();
    let rendered = payload.rendered_html.clone().unwrap_or_default();
    let pdf_options_json = payload
        .pdf_options
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO documents \
         (user_id, title, input_type, output_type, content, rendered_html, \
          theme, custom_css, pdf_options, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10) RETURNING id",
    )
    .bind(user.id)
    .bind(title)
    .bind(&payload.input_type)
    .bind(&payload.output)
    .bind(&payload.content)
    .bind(&rendered)
    .bind(payload.theme.as_deref())
    .bind(payload.custom_css.as_deref())
    .bind(pdf_options_json.as_deref())
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

    let now = now_seconds();
    let rendered = payload.rendered_html.clone().unwrap_or_default();
    let pdf_options_json = payload
        .pdf_options
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());

    let result = sqlx::query(
        "UPDATE documents SET title = ?1, input_type = ?2, output_type = ?3, content = ?4, \
         rendered_html = ?5, theme = ?6, custom_css = ?7, pdf_options = ?8, updated_at = ?9 \
         WHERE id = ?10 AND user_id = ?11",
    )
    .bind(title)
    .bind(&payload.input_type)
    .bind(&payload.output)
    .bind(&payload.content)
    .bind(&rendered)
    .bind(payload.theme.as_deref())
    .bind(payload.custom_css.as_deref())
    .bind(pdf_options_json.as_deref())
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
