use serde::Deserialize;
use worker::{Date, Env};

use crate::db::{self, Document, DocumentSummary, User};
use crate::error::ConvertError;

const MAX_TITLE: usize = 200;
const MAX_CONTENT: usize = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct SaveDocument {
    pub title: String,
    #[serde(rename = "type")]
    pub input_type: String,
    pub output: String,
    pub content: String,
    pub rendered_html: Option<String>,
}

pub async fn list(env: &Env, user: &User) -> Result<Vec<DocumentSummary>, ConvertError> {
    let result = db::d1(env)?
        .prepare(
            "SELECT id, user_id, title, input_type, output_type, content, rendered_html, \
                    created_at, updated_at \
             FROM documents WHERE user_id = ?1 ORDER BY updated_at DESC",
        )
        .bind(&[user.id.into()])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .all()
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?;
    let docs: Vec<Document> = result
        .results::<Document>()
        .map_err(|e| ConvertError::Database(e.to_string()))?;
    Ok(docs.iter().map(DocumentSummary::from).collect())
}

pub async fn get(env: &Env, user: &User, id: i64) -> Result<Document, ConvertError> {
    let doc = db::d1(env)?
        .prepare(
            "SELECT id, user_id, title, input_type, output_type, content, rendered_html, \
                    created_at, updated_at \
             FROM documents WHERE id = ?1 AND user_id = ?2",
        )
        .bind(&[id.into(), user.id.into()])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .first::<Document>(None)
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?;
    doc.ok_or(ConvertError::NotFound)
}

pub async fn save(env: &Env, user: &User, payload: &SaveDocument) -> Result<Document, ConvertError> {
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

    let now = (Date::now().as_millis() / 1000) as i64;
    let rendered = payload.rendered_html.clone().unwrap_or_default();
    let result = db::d1(env)?
        .prepare(
            "INSERT INTO documents \
             (user_id, title, input_type, output_type, content, rendered_html, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        )
        .bind(&[
            user.id.into(),
            title.into(),
            payload.input_type.clone().into(),
            payload.output.clone().into(),
            payload.content.clone().into(),
            rendered.into(),
            now.into(),
        ])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .run()
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?;

    let id = result
        .meta()
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .and_then(|m| m.last_row_id)
        .ok_or_else(|| ConvertError::Internal("no last_row_id after insert".into()))?;
    get(env, user, id).await
}

pub async fn delete(env: &Env, user: &User, id: i64) -> Result<(), ConvertError> {
    let result = db::d1(env)?
        .prepare("DELETE FROM documents WHERE id = ?1 AND user_id = ?2")
        .bind(&[id.into(), user.id.into()])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .run()
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?;
    let changes = result
        .meta()
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .and_then(|m| m.changes)
        .unwrap_or(0);
    if changes == 0 {
        return Err(ConvertError::NotFound);
    }
    Ok(())
}
