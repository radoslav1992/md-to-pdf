use std::net::SocketAddr;

use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use serde_json::json;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

mod admin;
mod auth;
mod convert;
mod crypto;
mod db;
mod documents;
mod error;
mod pdf;

use crate::db::AppState;
use crate::error::ConvertError;

const BODY_LIMIT_BYTES: usize = 8 * 1024 * 1024; // 8 MiB

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .compact()
        .init();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:/data/converter.db".into());
    let bind_addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8000".into())
        .parse()?;
    let chromium_bin = std::env::var("CHROMIUM_BIN").unwrap_or_else(|_| "chromium".into());
    let cookie_secure = std::env::var("COOKIE_SECURE")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    let admin_emails: Vec<String> = std::env::var("ADMIN_EMAILS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    tracing::info!(%database_url, %bind_addr, %chromium_bin, cookie_secure, "starting api");

    let pool = db::connect(&database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    let state = AppState {
        pool,
        admin_emails,
        cookie_secure,
        chromium_bin,
    };

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/signup", post(signup))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .route("/api/convert", post(convert_handler))
        .route("/api/documents", get(documents_list).post(documents_create))
        .route(
            "/api/documents/{id}",
            get(documents_get).delete(documents_delete),
        )
        .route("/api/admin/users", get(admin_users))
        .route("/api/admin/users/{id}/role", post(admin_update_role))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(addr = %bind_addr, "listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(json!({
        "ok": true,
        "service": "universal-document-converter-api",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "ok": false, "error": "not_found" })),
    )
}

// ---------- Auth ----------

async fn signup(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(creds): Json<auth::Credentials>,
) -> Result<(CookieJar, Json<serde_json::Value>), ConvertError> {
    let (user, token) = auth::signup(&state, &creds).await?;
    let jar = jar.add(auth::build_session_cookie(&state, token));
    Ok((jar, Json(json!({ "ok": true, "user": user.public() }))))
}

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(creds): Json<auth::Credentials>,
) -> Result<(CookieJar, Json<serde_json::Value>), ConvertError> {
    let (user, token) = auth::login(&state, &creds).await?;
    let jar = jar.add(auth::build_session_cookie(&state, token));
    Ok((jar, Json(json!({ "ok": true, "user": user.public() }))))
}

async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, Json<serde_json::Value>), ConvertError> {
    if let Some(c) = jar.get(auth::SESSION_COOKIE) {
        let _ = auth::logout(&state.pool, c.value()).await;
    }
    let jar = jar.add(auth::build_clear_cookie(&state));
    Ok((jar, Json(json!({ "ok": true }))))
}

async fn me(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let user = auth::current_user(&state, &jar).await?;
    Ok(Json(json!({
        "ok": true,
        "user": user.as_ref().map(|u| u.public()),
    })))
}

// ---------- Conversion ----------

async fn convert_handler(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<convert::ConvertRequest>,
) -> Result<Json<convert::ConvertResponse>, ConvertError> {
    let user = auth::current_user(&state, &jar).await?;
    let res = convert::run(&body, &state, user.as_ref()).await?;
    Ok(Json(res))
}

// ---------- Documents ----------

async fn documents_list(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let user = auth::require_user(&state, &jar).await?;
    let items = documents::list(&state.pool, &user).await?;
    Ok(Json(json!({ "ok": true, "items": items })))
}

async fn documents_create(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(payload): Json<documents::SaveDocument>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let user = auth::require_user(&state, &jar).await?;
    if !user.is_premium()
        && !matches!(
            payload.input_type.to_ascii_lowercase().as_str(),
            "markdown" | "md"
        )
    {
        return Err(ConvertError::PremiumRequired);
    }
    let doc = documents::save(&state.pool, &user, &payload).await?;
    Ok(Json(json!({ "ok": true, "document": doc })))
}

async fn documents_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let user = auth::require_user(&state, &jar).await?;
    let doc = documents::get(&state.pool, &user, id).await?;
    Ok(Json(json!({ "ok": true, "document": doc })))
}

async fn documents_delete(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let user = auth::require_user(&state, &jar).await?;
    documents::delete(&state.pool, &user, id).await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Admin ----------

async fn admin_users(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<serde_json::Value>, ConvertError> {
    auth::require_admin(&state, &jar).await?;
    let items = admin::list_users(&state.pool).await?;
    Ok(Json(json!({ "ok": true, "items": items })))
}

async fn admin_update_role(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Json(payload): Json<admin::UpdateRole>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let actor = auth::require_admin(&state, &jar).await?;
    let user = admin::update_role(&state.pool, &actor, id, &payload).await?;
    Ok(Json(json!({ "ok": true, "user": user })))
}
