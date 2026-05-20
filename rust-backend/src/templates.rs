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
    /// Stored as `INTEGER` in SQLite, `1` for public, `0` for private.
    /// A public template appears in `/templates/gallery` and is cloneable
    /// by any authenticated premium user.
    #[serde(serialize_with = "serialize_bool_int")]
    pub is_public: i64,
    pub clone_source_id: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

fn serialize_bool_int<S: serde::Serializer>(v: &i64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_bool(*v != 0)
}

#[derive(Debug, Serialize)]
pub struct GalleryTemplate {
    pub id: i64,
    pub name: String,
    pub theme: Option<String>,
    pub custom_css: Option<String>,
    pub pdf_options: Option<String>,
    /// Always 1 for gallery items. Repeated so the wire shape matches
    /// `Template` for shared rendering code on the frontend.
    pub is_public: bool,
    pub owner_id: i64,
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
    /// Optional on save. `None` means "don't change"; `Some(true)`
    /// publishes, `Some(false)` unpublishes.
    #[serde(default)]
    pub is_public: Option<bool>,
}

pub async fn list(pool: &SqlitePool, user: &User) -> Result<Vec<Template>, ConvertError> {
    let rows: Vec<Template> = sqlx::query_as(
        "SELECT id, user_id, name, theme, custom_css, pdf_options, is_public, clone_source_id, created_at, updated_at \
         FROM templates WHERE user_id = ?1 ORDER BY updated_at DESC",
    )
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &SqlitePool, user: &User, id: i64) -> Result<Template, ConvertError> {
    let row: Option<Template> = sqlx::query_as(
        "SELECT id, user_id, name, theme, custom_css, pdf_options, is_public, clone_source_id, created_at, updated_at \
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
    // `is_public` is tri-state: `None` keeps the current value, `Some`
    // sets it. Implemented in one statement with COALESCE so the
    // happy-path PATCH still touches a single row.
    let is_public_arg: Option<i64> = payload.is_public.map(|b| if b { 1 } else { 0 });
    let res = sqlx::query(
        "UPDATE templates SET name = ?1, theme = ?2, custom_css = ?3, pdf_options = ?4, \
         is_public = COALESCE(?5, is_public), updated_at = ?6 \
         WHERE id = ?7 AND user_id = ?8",
    )
    .bind(payload.name.trim())
    .bind(payload.theme.as_deref())
    .bind(payload.custom_css.as_deref())
    .bind(pdf_options_json.as_deref())
    .bind(is_public_arg)
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

/// List public templates from all users. Open to anonymous callers so
/// the gallery page works without an account (it's a browsing
/// experience; cloning is gated separately). Capped at 200 results to
/// keep payloads small; sort newest-updated first.
pub async fn gallery_list(pool: &SqlitePool) -> Result<Vec<GalleryTemplate>, ConvertError> {
    let rows: Vec<(i64, i64, String, Option<String>, Option<String>, Option<String>, i64, i64)> =
        sqlx::query_as(
            "SELECT id, user_id, name, theme, custom_css, pdf_options, created_at, updated_at \
             FROM templates WHERE is_public = 1 ORDER BY updated_at DESC LIMIT 200",
        )
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, owner_id, name, theme, custom_css, pdf_options, created_at, updated_at)| {
                GalleryTemplate {
                    id,
                    name,
                    theme,
                    custom_css,
                    pdf_options,
                    is_public: true,
                    owner_id,
                    created_at,
                    updated_at,
                }
            },
        )
        .collect())
}

/// Fetch one public template (anonymous callers OK). Returns 404 when
/// the template doesn't exist or isn't public; we deliberately collapse
/// the two cases so a probe can't enumerate private template ids.
pub async fn gallery_get(
    pool: &SqlitePool,
    id: i64,
) -> Result<GalleryTemplate, ConvertError> {
    let row: Option<(i64, i64, String, Option<String>, Option<String>, Option<String>, i64, i64)> =
        sqlx::query_as(
            "SELECT id, user_id, name, theme, custom_css, pdf_options, created_at, updated_at \
             FROM templates WHERE id = ?1 AND is_public = 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(
        |(id, owner_id, name, theme, custom_css, pdf_options, created_at, updated_at)| {
            GalleryTemplate {
                id,
                name,
                theme,
                custom_css,
                pdf_options,
                is_public: true,
                owner_id,
                created_at,
                updated_at,
            }
        },
    )
    .ok_or(ConvertError::NotFound)
}

/// Copy a public template into the caller's library. Premium-only —
/// templates themselves are a premium feature, so cloning shouldn't be
/// available to free accounts. We record `clone_source_id` so the
/// gallery view can later show "Cloned from <name>" or surface
/// attribution. The clone is private by default; the user can
/// re-publish it explicitly.
pub async fn gallery_clone(
    pool: &SqlitePool,
    user: &User,
    source_id: i64,
) -> Result<Template, ConvertError> {
    let src = gallery_get(pool, source_id).await?;
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
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO templates (user_id, name, theme, custom_css, pdf_options, \
                                 is_public, clone_source_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?7) RETURNING id",
    )
    .bind(user.id)
    .bind(format!("{} (copy)", src.name))
    .bind(src.theme.as_deref())
    .bind(src.custom_css.as_deref())
    .bind(src.pdf_options.as_deref())
    .bind(source_id)
    .bind(now)
    .fetch_one(pool)
    .await?;
    get(pool, user, row.0).await
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
