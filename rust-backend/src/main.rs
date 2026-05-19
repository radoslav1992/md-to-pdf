use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

mod admin;
mod api_keys;
mod auth;
mod batch;
mod convert;
mod crypto;
mod db;
mod doc_crypto;
mod document_versions;
mod documents;
mod enrichments;
mod error;
mod extract;
mod images;
mod jobs;
mod pandoc;
mod pdf;
mod render_cache;
mod shares;
mod templates;
mod themes;
mod usage;

use crate::db::AppState;
use crate::error::ConvertError;

const BODY_LIMIT_BYTES: usize = 32 * 1024 * 1024; // 32 MiB — covers 16 MiB premium input plus JSON envelope

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

    let render_cache_max_bytes: usize = std::env::var("RENDER_CACHE_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(render_cache::DEFAULT_MAX_BYTES);

    let state = AppState {
        pool,
        admin_emails,
        cookie_secure,
        chromium_bin,
        pdf_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(5)),
        render_cache: std::sync::Arc::new(std::sync::Mutex::new(
            render_cache::RenderCache::new(render_cache_max_bytes),
        )),
    };

    // Spawn the background job worker. One worker is enough on the
    // CX23-class hardware this is sized for; bump the PDF semaphore if you
    // need more parallel renders.
    {
        let worker_state = std::sync::Arc::new(state.clone());
        tokio::spawn(async move {
            jobs::run_worker(worker_state).await;
        });
    }

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/signup", post(signup))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .route("/api/convert", post(convert_handler))
        .route("/api/convert/batch", post(batch_handler))
        .route("/api/documents", get(documents_list).post(documents_create))
        .route(
            "/api/documents/{id}",
            get(documents_get)
                .patch(documents_update)
                .delete(documents_delete),
        )
        .route(
            "/api/templates",
            get(templates_list).post(templates_create),
        )
        .route(
            "/api/templates/{id}",
            get(templates_get)
                .patch(templates_update)
                .delete(templates_delete),
        )
        .route("/api/keys", get(keys_list).post(keys_create))
        .route("/api/keys/{id}", axum::routing::delete(keys_revoke))
        .route("/api/usage", get(usage_handler))
        .route("/api/jobs", get(jobs_list))
        .route("/api/jobs/convert", post(jobs_enqueue_convert))
        .route("/api/jobs/batch", post(jobs_enqueue_batch))
        .route("/api/jobs/{id}", get(jobs_get))
        .route("/api/jobs/{id}/cancel", post(jobs_cancel))
        .route(
            "/api/documents/{id}/versions",
            get(document_versions_list),
        )
        .route(
            "/api/documents/{id}/versions/{vid}",
            get(document_versions_get),
        )
        .route(
            "/api/documents/{id}/versions/{vid}/restore",
            post(document_versions_restore),
        )
        .route("/api/documents/{id}/decrypt", post(documents_decrypt))
        .route(
            "/api/documents/{id}/shares",
            get(shares_list).post(shares_create),
        )
        .route("/api/shares/{id}", axum::routing::delete(shares_revoke))
        .route("/api/share/{token}", get(share_meta).post(share_view))
        .route("/api/extract", post(extract_handler))
        .route("/api/images", get(images_list).post(images_upload))
        .route(
            "/api/images/{id}",
            get(images_serve).delete(images_delete),
        )
        .route("/api/admin/users", get(admin_users))
        .route("/api/admin/users/{id}/role", post(admin_update_role))
        .route("/api/admin/cache", get(admin_cache_stats))
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

fn authorization_header(headers: &HeaderMap) -> Option<&str> {
    headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()
}

async fn convert_handler(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<convert::ConvertRequest>,
) -> Result<Json<convert::ConvertResponse>, ConvertError> {
    let ctx = auth::resolve(&state, &jar, authorization_header(&headers)).await?;
    if let Some(ctx) = &ctx {
        usage::reserve(&state.pool, &ctx.user, ctx.api_key_id, "convert", 1).await?;
    }
    let res = convert::run(&body, &state, ctx.as_ref().map(|c| &c.user)).await?;
    Ok(Json(res))
}

// ---------- Batch ----------

#[derive(Debug, Deserialize)]
struct BatchQuery {
    #[serde(default)]
    format: Option<String>,
}

async fn batch_handler(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(query): Query<BatchQuery>,
    Json(body): Json<batch::BatchRequest>,
) -> Result<Response, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    if !ctx.user.is_premium() {
        return Err(ConvertError::PremiumRequired);
    }
    batch::validate(&body)?;
    usage::reserve(
        &state.pool,
        &ctx.user,
        ctx.api_key_id,
        "batch",
        body.items.len() as i64,
    )
    .await?;
    let result = batch::run(&body, &state, &ctx.user).await?;
    let want_zip = query.format.as_deref() == Some("zip");
    if want_zip {
        let bytes = batch::to_zip(&result)?;
        let mut resp = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/zip")
            .header("content-disposition", "attachment; filename=\"batch.zip\"")
            .body(Body::from(bytes))
            .map_err(|e| ConvertError::Internal(format!("zip response: {e}")))?;
        if let Some(delivered) = result.webhook_delivered {
            if let Ok(value) = axum::http::HeaderValue::from_str(&delivered.to_string()) {
                resp.headers_mut().insert("x-udc-webhook-delivered", value);
            }
        }
        Ok(resp)
    } else {
        Ok((StatusCode::OK, Json(result)).into_response())
    }
}

// ---------- Documents ----------

async fn documents_list(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(filter): Query<documents::ListFilter>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let user = auth::require_user(&state, &jar).await?;
    let items = documents::list(&state.pool, &user, &filter).await?;
    let folders = documents::list_folders(&state.pool, &user).await?;
    let tags = documents::list_tags(&state.pool, &user).await?;
    Ok(Json(
        json!({ "ok": true, "items": items, "folders": folders, "tags": tags }),
    ))
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

async fn documents_update(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Json(payload): Json<documents::SaveDocument>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let user = auth::require_user(&state, &jar).await?;
    let doc = documents::update(&state.pool, &user, id, &payload).await?;
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

// ---------- Templates ----------

fn require_premium(user: &crate::db::User) -> Result<(), ConvertError> {
    if user.is_premium() {
        Ok(())
    } else {
        Err(ConvertError::PremiumRequired)
    }
}

async fn templates_list(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    let items = templates::list(&state.pool, &ctx.user).await?;
    Ok(Json(json!({ "ok": true, "items": items })))
}

async fn templates_get(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    let t = templates::get(&state.pool, &ctx.user, id).await?;
    Ok(Json(json!({ "ok": true, "template": t })))
}

async fn templates_create(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(payload): Json<templates::SaveTemplate>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    let t = templates::create(&state.pool, &ctx.user, &payload).await?;
    Ok(Json(json!({ "ok": true, "template": t })))
}

async fn templates_update(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(payload): Json<templates::SaveTemplate>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    let t = templates::update(&state.pool, &ctx.user, id, &payload).await?;
    Ok(Json(json!({ "ok": true, "template": t })))
}

async fn templates_delete(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    templates::delete(&state.pool, &ctx.user, id).await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- API keys ----------

async fn keys_list(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    let items = api_keys::list(&state.pool, &ctx.user).await?;
    Ok(Json(json!({ "ok": true, "items": items })))
}

async fn keys_create(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(payload): Json<api_keys::CreateKey>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    let (key, plaintext) = api_keys::create(&state.pool, &ctx.user, &payload).await?;
    Ok(Json(json!({ "ok": true, "key": key, "plaintext": plaintext })))
}

async fn keys_revoke(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    api_keys::revoke(&state.pool, &ctx.user, id).await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Usage ----------

async fn usage_handler(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    let summary = usage::summary(&state.pool, &ctx.user).await?;
    Ok(Json(json!({ "ok": true, "usage": summary })))
}

// ---------- Jobs ----------

async fn jobs_list(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    let items = jobs::list(&state.pool, &ctx.user).await?;
    Ok(Json(json!({ "ok": true, "items": items })))
}

async fn jobs_get(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    let job = jobs::get(&state.pool, &ctx.user, id).await?;
    Ok(Json(json!({ "ok": true, "job": job })))
}

async fn jobs_cancel(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    jobs::cancel(&state.pool, &ctx.user, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn jobs_enqueue_convert(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<convert::ConvertRequest>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    usage::reserve(&state.pool, &ctx.user, ctx.api_key_id, "convert", 1).await?;
    let job = jobs::enqueue_convert(&state.pool, &ctx.user, ctx.api_key_id, &body).await?;
    Ok(Json(json!({ "ok": true, "job_id": job.id, "status": job.status })))
}

async fn jobs_enqueue_batch(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<batch::BatchRequest>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    batch::validate(&body)?;
    usage::reserve(
        &state.pool,
        &ctx.user,
        ctx.api_key_id,
        "batch",
        body.items.len() as i64,
    )
    .await?;
    let job = jobs::enqueue_batch(&state.pool, &ctx.user, ctx.api_key_id, &body).await?;
    Ok(Json(json!({ "ok": true, "job_id": job.id, "status": job.status })))
}

// ---------- Document versions ----------

async fn document_versions_list(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    let items = document_versions::list(&state.pool, &ctx.user, id).await?;
    Ok(Json(json!({ "ok": true, "items": items })))
}

async fn document_versions_get(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path((id, vid)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    let version = document_versions::get(&state.pool, &ctx.user, id, vid).await?;
    Ok(Json(json!({ "ok": true, "version": version })))
}

async fn document_versions_restore(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path((id, vid)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    let doc = documents::restore_version(&state.pool, &ctx.user, id, vid).await?;
    Ok(Json(json!({ "ok": true, "document": doc })))
}

// ---------- Encrypted documents ----------

#[derive(Debug, Deserialize)]
struct DecryptRequest {
    password: String,
}

async fn documents_decrypt(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(payload): Json<DecryptRequest>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    let plaintext = documents::decrypt_content(&state.pool, &ctx.user, id, &payload.password).await?;
    Ok(Json(json!({ "ok": true, "content": plaintext })))
}

// ---------- Shares ----------

async fn shares_list(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    let items = shares::list_for_document(&state.pool, &ctx.user, id).await?;
    Ok(Json(json!({ "ok": true, "items": items })))
}

async fn shares_create(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(payload): Json<shares::CreateShare>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    let (link, plaintext) = shares::create(&state.pool, &ctx.user, id, &payload).await?;
    Ok(Json(json!({
        "ok": true,
        "share": link,
        "url": format!("/s/{plaintext}"),
        "token": plaintext,
    })))
}

async fn shares_revoke(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    shares::revoke(&state.pool, &ctx.user, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn share_meta(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let link = shares::lookup(&state.pool, &token).await?;
    Ok(Json(json!({
        "ok": true,
        "format": link.format,
        "requires_password": link.requires_password(),
        "expires_at": link.expires_at,
    })))
}

#[derive(Debug, Deserialize, Default)]
struct ShareViewRequest {
    #[serde(default)]
    password: Option<String>,
}

async fn share_view(
    State(state): State<AppState>,
    Path(token): Path<String>,
    body: Option<Json<ShareViewRequest>>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let payload = body.map(|Json(b)| b).unwrap_or_default();
    let link = shares::lookup(&state.pool, &token).await?;
    shares::verify_password(&link, payload.password.as_deref())?;
    // Look up the document directly (we already have user_id on the link).
    let doc: Option<crate::db::Document> = sqlx::query_as(
        "SELECT id, user_id, title, input_type, output_type, content, rendered_html, \
                theme, custom_css, pdf_options, is_encrypted, encryption_salt, folder, tags, created_at, updated_at \
         FROM documents WHERE id = ?1 AND user_id = ?2",
    )
    .bind(link.document_id)
    .bind(link.user_id)
    .fetch_optional(&state.pool)
    .await?;
    let doc = doc.ok_or(ConvertError::NotFound)?;
    if doc.is_encrypted != 0 {
        return Err(ConvertError::BadRequest(
            "encrypted documents cannot be shared via public link".into(),
        ));
    }
    shares::bump_view_count(&state.pool, link.id).await;
    Ok(Json(json!({
        "ok": true,
        "title": doc.title,
        "format": link.format,
        "rendered_html": doc.rendered_html,
    })))
}

// ---------- PDF → Markdown extraction ----------

async fn extract_handler(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(payload): Json<extract::ExtractRequest>,
) -> Result<Json<extract::ExtractResponse>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    usage::reserve(&state.pool, &ctx.user, ctx.api_key_id, "extract", 1).await?;
    let res = extract::run(&payload).await?;
    Ok(Json(res))
}

// ---------- Images ----------

async fn images_list(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    let items = images::list(&state.pool, &ctx.user).await?;
    Ok(Json(json!({ "ok": true, "items": items })))
}

async fn images_upload(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(payload): Json<images::UploadImage>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    let res = images::upload(&state.pool, &ctx.user, &payload).await?;
    Ok(Json(json!({ "ok": true, "image": res })))
}

async fn images_serve(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    let (meta, bytes) = images::fetch(&state.pool, &ctx.user, id).await?;
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", meta.content_type)
        .header("cache-control", "private, max-age=86400")
        .header("content-length", bytes.len().to_string())
        .body(Body::from(bytes))
        .map_err(|e| ConvertError::Internal(format!("image response: {e}")))?;
    Ok(resp)
}

async fn images_delete(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    images::delete(&state.pool, &ctx.user, id).await?;
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

async fn admin_cache_stats(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<serde_json::Value>, ConvertError> {
    auth::require_admin(&state, &jar).await?;
    let stats = state.render_cache.lock().expect("render cache poisoned").stats();
    Ok(Json(json!({ "ok": true, "cache": stats })))
}
