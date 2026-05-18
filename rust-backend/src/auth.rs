use serde::Deserialize;
use worker::{Date, Env, Headers, Request};

use crate::crypto;
use crate::db::{self, User};
use crate::error::ConvertError;

const SESSION_COOKIE: &str = "session";
const SESSION_TTL_SECONDS: i64 = 60 * 60 * 24 * 30; // 30 days

#[derive(Debug, Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

pub async fn signup(env: &Env, creds: &Credentials) -> Result<(User, String), ConvertError> {
    let email = normalize_email(&creds.email)?;
    validate_password(&creds.password)?;

    let db = db::d1(env)?;
    let existing = db
        .prepare("SELECT id FROM users WHERE email = ?1")
        .bind(&[email.clone().into()])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .first::<serde_json::Value>(None)
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?;
    if existing.is_some() {
        return Err(ConvertError::Conflict("email already registered".into()));
    }

    let (hash, salt) = crypto::hash_password(&creds.password)?;
    let role = bootstrap_role(env, &email);
    let now = now_seconds();

    let result = db
        .prepare(
            "INSERT INTO users (email, password_hash, password_salt, role, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(&[
            email.clone().into(),
            hash.into(),
            salt.into(),
            role.into(),
            now.into(),
        ])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .run()
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?;

    let user_id = result
        .meta()
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .and_then(|m| m.last_row_id)
        .ok_or_else(|| ConvertError::Internal("no last_row_id after insert".into()))?;

    let user = load_user_by_id(env, user_id).await?;
    let token = create_session(env, user.id).await?;
    Ok((user, token))
}

pub async fn login(env: &Env, creds: &Credentials) -> Result<(User, String), ConvertError> {
    let email = normalize_email(&creds.email)?;
    let db = db::d1(env)?;
    let row = db
        .prepare(
            "SELECT id, email, password_hash, password_salt, role, created_at \
             FROM users WHERE email = ?1",
        )
        .bind(&[email.into()])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .first::<User>(None)
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?;

    let user = row.ok_or(ConvertError::Unauthorized)?;
    if !crypto::verify_password(&creds.password, &user.password_salt, &user.password_hash) {
        return Err(ConvertError::Unauthorized);
    }
    let token = create_session(env, user.id).await?;
    Ok((user, token))
}

pub async fn logout(env: &Env, token: &str) -> Result<(), ConvertError> {
    let db = db::d1(env)?;
    db.prepare("DELETE FROM sessions WHERE token = ?1")
        .bind(&[token.into()])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .run()
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?;
    Ok(())
}

pub async fn current_user(env: &Env, req: &Request) -> Result<Option<User>, ConvertError> {
    let token = match cookie_value(req, SESSION_COOKIE) {
        Some(t) => t,
        None => return Ok(None),
    };
    let db = db::d1(env)?;
    let session = db
        .prepare("SELECT user_id, expires_at FROM sessions WHERE token = ?1")
        .bind(&[token.clone().into()])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .first::<SessionRow>(None)
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?;
    let Some(session) = session else {
        return Ok(None);
    };
    if session.expires_at < now_seconds() {
        let _ = db
            .prepare("DELETE FROM sessions WHERE token = ?1")
            .bind(&[token.into()])
            .map_err(|e| ConvertError::Database(e.to_string()))?
            .run()
            .await;
        return Ok(None);
    }
    Ok(Some(load_user_by_id(env, session.user_id).await?))
}

pub async fn require_user(env: &Env, req: &Request) -> Result<User, ConvertError> {
    current_user(env, req)
        .await?
        .ok_or(ConvertError::Unauthorized)
}

pub async fn require_admin(env: &Env, req: &Request) -> Result<User, ConvertError> {
    let user = require_user(env, req).await?;
    if !user.is_admin() {
        return Err(ConvertError::Forbidden("admin role required".into()));
    }
    Ok(user)
}

pub fn build_session_cookie(env: &Env, token: &str) -> String {
    let secure = env
        .var("COOKIE_SECURE")
        .ok()
        .map(|v| v.to_string() == "true")
        .unwrap_or(true);
    let secure_attr = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECONDS}{secure_attr}"
    )
}

pub fn build_clear_cookie(env: &Env) -> String {
    let secure = env
        .var("COOKIE_SECURE")
        .ok()
        .map(|v| v.to_string() == "true")
        .unwrap_or(true);
    let secure_attr = if secure { "; Secure" } else { "" };
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_attr}")
}

pub fn attach_cookie(headers: &mut Headers, cookie: &str) -> Result<(), ConvertError> {
    headers
        .append("set-cookie", cookie)
        .map_err(|e| ConvertError::Internal(e.to_string()))
}

async fn create_session(env: &Env, user_id: i64) -> Result<String, ConvertError> {
    let token = crypto::random_token()?;
    let expires_at = now_seconds() + SESSION_TTL_SECONDS;
    db::d1(env)?
        .prepare("INSERT INTO sessions (token, user_id, expires_at) VALUES (?1, ?2, ?3)")
        .bind(&[token.clone().into(), user_id.into(), expires_at.into()])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .run()
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?;
    Ok(token)
}

async fn load_user_by_id(env: &Env, id: i64) -> Result<User, ConvertError> {
    db::d1(env)?
        .prepare(
            "SELECT id, email, password_hash, password_salt, role, created_at \
             FROM users WHERE id = ?1",
        )
        .bind(&[id.into()])
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .first::<User>(None)
        .await
        .map_err(|e| ConvertError::Database(e.to_string()))?
        .ok_or(ConvertError::NotFound)
}

fn bootstrap_role(env: &Env, email: &str) -> String {
    let list = env
        .var("ADMIN_EMAILS")
        .ok()
        .map(|v| v.to_string())
        .unwrap_or_default();
    let is_admin = list
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .any(|s| s == email);
    if is_admin {
        "admin".to_string()
    } else {
        "free".to_string()
    }
}

fn normalize_email(input: &str) -> Result<String, ConvertError> {
    let trimmed = input.trim().to_ascii_lowercase();
    if trimmed.is_empty() || !trimmed.contains('@') || trimmed.len() > 320 {
        return Err(ConvertError::BadRequest("invalid email".into()));
    }
    Ok(trimmed)
}

fn validate_password(password: &str) -> Result<(), ConvertError> {
    if password.len() < 8 {
        return Err(ConvertError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }
    if password.len() > 1024 {
        return Err(ConvertError::BadRequest("password too long".into()));
    }
    Ok(())
}

fn cookie_value(req: &Request, name: &str) -> Option<String> {
    let header = req.headers().get("cookie").ok().flatten()?;
    for part in header.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k == name && !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn now_seconds() -> i64 {
    (Date::now().as_millis() / 1000) as i64
}

#[derive(Debug, serde::Deserialize)]
struct SessionRow {
    user_id: i64,
    expires_at: i64,
}
