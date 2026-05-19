//! Reusable styling templates. A template captures a theme name, custom
//! CSS, and PDF options; it can be expanded into a `ConvertRequest` so the
//! same look-and-feel can be applied across many documents.
//!
//! Premium-only — free users only ever use the default style.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::{now_seconds, User};
use crate::error::ConvertError;

const MAX_NAME: usize = 120;
const MAX_PER_USER: i64 = 200;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Template {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub theme: Option<String>,
    pub custom_css: Option<String>,
    pub pdf_options: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct SaveTemplate {
    pub name: String,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub custom_css: Option<String>,
    #[serde(default)]
    pub pdf_options: Option<serde_json::Value>,
}

pub async fn list(pool: &SqlitePool, user: &User) -> Result<Vec<Template>, ConvertError> {
    let rows: Vec<Template> = sqlx::query_as(
        "SELECT id, user_id, name, theme, custom_css, pdf_options, created_at, updated_at \
         FROM templates WHERE user_id = ?1 ORDER BY updated_at DESC",
    )
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &SqlitePool, user: &User, id: i64) -> Result<Template, ConvertError> {
    let row: Option<Template> = sqlx::query_as(
        "SELECT id, user_id, name, theme, custom_css, pdf_options, created_at, updated_at \
         FROM templates WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(pool)
    .await?;
    row.ok_or(ConvertError::NotFound)
}

pub async fn create(
    pool: &SqlitePool,
    user: &User,
    payload: &SaveTemplate,
) -> Result<Template, ConvertError> {
    validate(payload)?;
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM templates WHERE user_id = ?1")
            .bind(user.id)
            .fetch_one(pool)
            .await?;
    if count.0 >= MAX_PER_USER {
        return Err(ConvertError::Conflict(format!(
            "template limit reached ({MAX_PER_USER} per user)"
        )));
    }
    let now = now_seconds();
    let pdf_options_json = payload
        .pdf_options
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO templates (user_id, name, theme, custom_css, pdf_options, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) RETURNING id",
    )
    .bind(user.id)
    .bind(payload.name.trim())
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
    payload: &SaveTemplate,
) -> Result<Template, ConvertError> {
    validate(payload)?;
    let now = now_seconds();
    let pdf_options_json = payload
        .pdf_options
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());
    let res = sqlx::query(
        "UPDATE templates SET name = ?1, theme = ?2, custom_css = ?3, pdf_options = ?4, \
         updated_at = ?5 WHERE id = ?6 AND user_id = ?7",
    )
    .bind(payload.name.trim())
    .bind(payload.theme.as_deref())
    .bind(payload.custom_css.as_deref())
    .bind(pdf_options_json.as_deref())
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
    let res = sqlx::query("DELETE FROM templates WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(user.id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ConvertError::NotFound);
    }
    Ok(())
}

fn validate(payload: &SaveTemplate) -> Result<(), ConvertError> {
    let name = payload.name.trim();
    if name.is_empty() || name.len() > MAX_NAME {
        return Err(ConvertError::BadRequest(
            "template name must be 1..=120 characters".into(),
        ));
    }
    Ok(())
}
