use serde::Deserialize;
use worker::Env;

use crate::db::{self, PublicUser, User};
use crate::error::ConvertError;

const ALLOWED_ROLES: &[&str] = &["free", "premium", "admin"];

#[derive(Debug, Deserialize)]
pub struct UpdateRole {
    pub role: String,
}

pub async fn list_users(env: &Env) -> Result<Vec<PublicUser>, ConvertError> {
    let result = db::d1(env)?
        .prepare(
            "SELECT id, email, password_hash, password_salt, role, created_at \
             FROM users ORDER BY created_at DESC",
        )
        .all()
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?;
    let users: Vec<User> = result
        .results::<User>()
        .map_err(|e| ConvertError::Database(e.to_string()))?;
    Ok(users.iter().map(User::public).collect())
}

pub async fn update_role(
    env: &Env,
    actor: &User,
    target_id: i64,
    update: &UpdateRole,
) -> Result<PublicUser, ConvertError> {
    let role = update.role.trim().to_ascii_lowercase();
    if !ALLOWED_ROLES.contains(&role.as_str()) {
        return Err(ConvertError::BadRequest(format!(
            "role must be one of: {}",
            ALLOWED_ROLES.join(", ")
        )));
    }
    // Guard against an admin demoting themselves and locking the system out.
    if actor.id == target_id && role != "admin" {
        return Err(ConvertError::Forbidden(
            "admins cannot remove their own admin role".into(),
        ));
    }
    let db = db::d1(env)?;
    let result = db
        .prepare("UPDATE users SET role = ?1 WHERE id = ?2")
        .bind(&[role.into(), target_id.into()])
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

    let updated = db
        .prepare(
            "SELECT id, email, password_hash, password_salt, role, created_at \
             FROM users WHERE id = ?1",
        )
        .bind(&[target_id.into()])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .first::<User>(None)
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .ok_or(ConvertError::NotFound)?;
    Ok(updated.public())
}
