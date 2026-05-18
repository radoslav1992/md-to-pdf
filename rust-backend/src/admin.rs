use serde::Deserialize;
use sqlx::SqlitePool;

use crate::db::{PublicUser, User};
use crate::error::ConvertError;

const ALLOWED_ROLES: &[&str] = &["free", "premium", "admin"];

#[derive(Debug, Deserialize)]
pub struct UpdateRole {
    pub role: String,
}

pub async fn list_users(pool: &SqlitePool) -> Result<Vec<PublicUser>, ConvertError> {
    let users: Vec<User> = sqlx::query_as(
        "SELECT id, email, password_hash, password_salt, role, created_at \
         FROM users ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(users.iter().map(User::public).collect())
}

pub async fn update_role(
    pool: &SqlitePool,
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
    if actor.id == target_id && role != "admin" {
        return Err(ConvertError::Forbidden(
            "admins cannot remove their own admin role".into(),
        ));
    }
    let result = sqlx::query("UPDATE users SET role = ?1 WHERE id = ?2")
        .bind(&role)
        .bind(target_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ConvertError::NotFound);
    }

    let updated: User = sqlx::query_as(
        "SELECT id, email, password_hash, password_salt, role, created_at \
         FROM users WHERE id = ?1",
    )
    .bind(target_id)
    .fetch_optional(pool)
    .await?
    .ok_or(ConvertError::NotFound)?;
    Ok(updated.public())
}
