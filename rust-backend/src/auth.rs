use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::crypto;
use crate::db::{now_seconds, AppState, User};
use crate::error::ConvertError;

pub const SESSION_COOKIE: &str = "session";
const SESSION_TTL_SECONDS: i64 = 60 * 60 * 24 * 30; // 30 days

#[derive(Debug, Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

pub async fn signup(
    state: &AppState,
    creds: &Credentials,
) -> Result<(User, String), ConvertError> {
    let email = normalize_email(&creds.email)?;
    validate_password(&creds.password)?;

    let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?1")
        .bind(&email)
        .fetch_optional(&state.pool)
        .await?;
    if existing.is_some() {
        return Err(ConvertError::Conflict("email already registered".into()));
    }

    let (hash, salt) = crypto::hash_password(&creds.password)?;
    let role = bootstrap_role(state, &email);
    let now = now_seconds();

    let row: (i64,) = sqlx::query_as(
        "INSERT INTO users (email, password_hash, password_salt, role, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
    )
    .bind(&email)
    .bind(&hash)
    .bind(&salt)
    .bind(&role)
    .bind(now)
    .fetch_one(&state.pool)
    .await?;

    let user = load_user_by_id(&state.pool, row.0).await?;
    let token = create_session(&state.pool, user.id).await?;
    Ok((user, token))
}

pub async fn login(
    state: &AppState,
    creds: &Credentials,
) -> Result<(User, String), ConvertError> {
    let email = normalize_email(&creds.email)?;
    let user: Option<User> = sqlx::query_as(
        "SELECT id, email, password_hash, password_salt, role, created_at \
         FROM users WHERE email = ?1",
    )
    .bind(&email)
    .fetch_optional(&state.pool)
    .await?;
    let user = user.ok_or(ConvertError::Unauthorized)?;
    if !crypto::verify_password(&creds.password, &user.password_salt, &user.password_hash) {
        return Err(ConvertError::Unauthorized);
    }
    let token = create_session(&state.pool, user.id).await?;
    Ok((user, token))
}

pub async fn logout(pool: &SqlitePool, token: &str) -> Result<(), ConvertError> {
    sqlx::query("DELETE FROM sessions WHERE token = ?1")
        .bind(token)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn current_user(state: &AppState, jar: &CookieJar) -> Result<Option<User>, ConvertError> {
    let token = match jar.get(SESSION_COOKIE) {
        Some(c) => c.value().to_string(),
        None => return Ok(None),
    };
    let session: Option<(i64, i64)> =
        sqlx::query_as("SELECT user_id, expires_at FROM sessions WHERE token = ?1")
            .bind(&token)
            .fetch_optional(&state.pool)
            .await?;
    let Some((user_id, expires_at)) = session else {
        return Ok(None);
    };
    if expires_at < now_seconds() {
        let _ = sqlx::query("DELETE FROM sessions WHERE token = ?1")
            .bind(&token)
            .execute(&state.pool)
            .await;
        return Ok(None);
    }
    Ok(Some(load_user_by_id(&state.pool, user_id).await?))
}

pub async fn require_user(state: &AppState, jar: &CookieJar) -> Result<User, ConvertError> {
    current_user(state, jar)
        .await?
        .ok_or(ConvertError::Unauthorized)
}

pub async fn require_admin(state: &AppState, jar: &CookieJar) -> Result<User, ConvertError> {
    let user = require_user(state, jar).await?;
    if !user.is_admin() {
        return Err(ConvertError::Forbidden("admin role required".into()));
    }
    Ok(user)
}

pub fn build_session_cookie(state: &AppState, token: String) -> Cookie<'static> {
    let mut c = Cookie::new(SESSION_COOKIE, token);
    c.set_path("/");
    c.set_http_only(true);
    c.set_secure(state.cookie_secure);
    c.set_same_site(SameSite::Lax);
    c.set_max_age(time::Duration::seconds(SESSION_TTL_SECONDS));
    c
}

pub fn build_clear_cookie(state: &AppState) -> Cookie<'static> {
    let mut c = Cookie::new(SESSION_COOKIE, "");
    c.set_path("/");
    c.set_http_only(true);
    c.set_secure(state.cookie_secure);
    c.set_same_site(SameSite::Lax);
    c.set_max_age(time::Duration::seconds(0));
    c
}

async fn create_session(pool: &SqlitePool, user_id: i64) -> Result<String, ConvertError> {
    let token = crypto::random_token();
    let expires_at = now_seconds() + SESSION_TTL_SECONDS;
    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES (?1, ?2, ?3)")
        .bind(&token)
        .bind(user_id)
        .bind(expires_at)
        .execute(pool)
        .await?;
    Ok(token)
}

async fn load_user_by_id(pool: &SqlitePool, id: i64) -> Result<User, ConvertError> {
    sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, password_salt, role, created_at \
         FROM users WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(ConvertError::NotFound)
}

fn bootstrap_role(state: &AppState, email: &str) -> String {
    if state.admin_emails.iter().any(|a| a == email) {
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
