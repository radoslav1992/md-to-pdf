use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
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
mod pdf_tools;
mod rate_limit;
mod render_cache;
mod shares;
mod templates;
mod themes;
mod usage;

use crate::db::AppState;
use crate::error::ConvertError;
use crate::rate_limit::{Decision, Identity, RateLimiter, Tier};

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

    let chromium_pool_size: usize = std::env::var("CHROMIUM_POOL_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n: &usize| *n >= 1 && *n <= 32)
        .unwrap_or(5);
    let chromium_pool_root: std::path::PathBuf = std::env::var("CHROMIUM_POOL_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("udc-chromium-pool"));
    let chromium_pool = std::sync::Arc::new(
        pdf::ChromiumSlotPool::new(chromium_pool_size, &chromium_pool_root)?,
    );
    tracing::info!(
        size = chromium_pool_size,
        root = %chromium_pool_root.display(),
        "chromium warm pool ready"
    );

    let rate_limit_buckets: usize = std::env::var("RATE_LIMIT_MAX_BUCKETS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);

    let state = AppState {
        pool,
        admin_emails,
        cookie_secure,
        chromium_bin,
        chromium_pool,
        render_cache: std::sync::Arc::new(std::sync::Mutex::new(
            render_cache::RenderCache::new(render_cache_max_bytes),
        )),
        rate_limiter: std::sync::Arc::new(RateLimiter::new(rate_limit_buckets)),
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

    // The API surface is defined once (with no `/api` prefix) and mounted
    // at both `/api` (legacy) and `/api/v1` (versioned). Future breaking
    // changes can land at `/api/v2` without disturbing existing clients.
    let app = Router::new()
        .nest("/api", build_api_router(state.clone()))
        .nest("/api/v1", build_api_router(state.clone()))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(addr = %bind_addr, "listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Build the routes that live under both `/api` and `/api/v1`. Defined
/// once so the two mount points stay in lock-step automatically.
fn build_api_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/openapi.yaml", get(openapi_yaml))
        .route("/openapi.json", get(openapi_json))
        .route("/auth/signup", post(signup))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route("/convert", post(convert_handler))
        .route("/convert/batch", post(batch_handler))
        .route("/documents", get(documents_list).post(documents_create))
        .route(
            "/documents/{id}",
            get(documents_get)
                .patch(documents_update)
                .delete(documents_delete),
        )
        .route("/templates", get(templates_list).post(templates_create))
        .route(
            "/templates/{id}",
            get(templates_get)
                .patch(templates_update)
                .delete(templates_delete),
        )
        .route("/keys", get(keys_list).post(keys_create))
        .route("/keys/{id}", axum::routing::delete(keys_revoke))
        .route("/usage", get(usage_handler))
        .route("/jobs", get(jobs_list))
        .route("/jobs/convert", post(jobs_enqueue_convert))
        .route("/jobs/batch", post(jobs_enqueue_batch))
        .route("/jobs/{id}", get(jobs_get))
        .route("/jobs/{id}/cancel", post(jobs_cancel))
        .route("/documents/{id}/versions", get(document_versions_list))
        .route(
            "/documents/{id}/versions/{vid}",
            get(document_versions_get),
        )
        .route(
            "/documents/{id}/versions/{vid}/restore",
            post(document_versions_restore),
        )
        .route("/documents/{id}/decrypt", post(documents_decrypt))
        .route(
            "/documents/{id}/shares",
            get(shares_list).post(shares_create),
        )
        .route("/shares/{id}", axum::routing::delete(shares_revoke))
        .route("/share/{token}", get(share_meta).post(share_view))
        .route("/extract", post(extract_handler))
        .route("/pdf/merge", post(pdf_merge_handler))
        .route("/pdf/split", post(pdf_split_handler))
        .route("/pdf/compress", post(pdf_compress_handler))
        .route("/pdf/watermark", post(pdf_watermark_handler))
        .route("/pdf/encrypt", post(pdf_encrypt_handler))
        .route("/images", get(images_list).post(images_upload))
        .route(
            "/images/{id}",
            get(images_serve).delete(images_delete),
        )
        .route("/admin/users", get(admin_users))
        .route("/admin/users/{id}/role", post(admin_update_role))
        .route("/admin/cache", get(admin_cache_stats))
        .route("/admin/ratelimit", get(admin_ratelimit_stats))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(json!({
        "ok": true,
        "service": "universal-document-converter-api",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

// ---------- OpenAPI ----------

/// The hand-written spec, baked into the binary at compile time.
const OPENAPI_YAML: &str = include_str!("../openapi/openapi.yaml");

async fn openapi_yaml() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/yaml; charset=utf-8")
        .header("cache-control", "public, max-age=300")
        .body(Body::from(OPENAPI_YAML))
        .expect("static body")
}

async fn openapi_json() -> Response {
    // Convert YAML → JSON on the fly. Cheap (file is < 50 KB) and means
    // we only have one source of truth.
    let value: serde_yaml::Value =
        serde_yaml::from_str(OPENAPI_YAML).expect("openapi yaml parses at build time");
    let json = serde_json::to_vec(&value).expect("openapi value serialises to JSON");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("cache-control", "public, max-age=300")
        .body(Body::from(json))
        .expect("static body")
}

// ---------- Rate limiting ----------

/// Paths that bypass the rate limiter — pure status probes that must
/// always answer so monitors don't flag the service as down under burst
/// load.
const RATE_LIMIT_EXEMPT_PATHS: &[&str] = &["/health", "/openapi.yaml", "/openapi.json"];

async fn rate_limit_middleware(
    State(state): State<AppState>,
    jar: CookieJar,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if RATE_LIMIT_EXEMPT_PATHS.iter().any(|p| path == *p) {
        return next.run(req).await;
    }

    // Resolve the caller (cookie or bearer key). Failures are treated as
    // anonymous — auth errors are the handler's job to return.
    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .map(String::from);
    let ctx = auth::resolve(&state, &jar, auth_header.as_deref())
        .await
        .ok()
        .flatten();

    let (identity, tier) = if let Some(ctx) = ctx.as_ref() {
        let id = if let Some(kid) = ctx.api_key_id {
            Identity::ApiKey(kid)
        } else {
            Identity::User(ctx.user.id)
        };
        (id, Tier::for_role(&ctx.user.role))
    } else {
        // Prefer the X-Forwarded-For first hop (Caddy injects it), fall
        // back to the direct peer addr from ConnectInfo extensions,
        // finally to a shared "anon" bucket so a missing header doesn't
        // crash through unlimited.
        let ip = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim().to_string())
            .or_else(|| {
                req.extensions()
                    .get::<ConnectInfo<SocketAddr>>()
                    .map(|c| c.0.ip().to_string())
            })
            .unwrap_or_else(|| "anon".to_string());
        (Identity::Ip(ip), Tier::ANON)
    };

    let decision = state.rate_limiter.check(identity, tier);
    match decision {
        Decision::Allow { remaining, capacity } => {
            let mut resp = next.run(req).await;
            add_rate_headers(resp.headers_mut(), remaining, capacity);
            resp
        }
        Decision::Limited {
            retry_after_secs,
            capacity,
        } => {
            let body = json!({
                "ok": false,
                "error": "rate limit exceeded",
                "retry_after_secs": retry_after_secs,
            });
            let mut resp = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
            let h = resp.headers_mut();
            h.insert(
                "retry-after",
                HeaderValue::from_str(&retry_after_secs.to_string())
                    .unwrap_or(HeaderValue::from_static("60")),
            );
            add_rate_headers(h, 0, capacity);
            resp
        }
    }
}

fn add_rate_headers(headers: &mut HeaderMap, remaining: u32, capacity: u32) {
    if let Ok(v) = HeaderValue::from_str(&capacity.to_string()) {
        headers.insert("x-ratelimit-limit", v);
    }
    if let Ok(v) = HeaderValue::from_str(&remaining.to_string()) {
        headers.insert("x-ratelimit-remaining", v);
    }
    if let Ok(v) = HeaderValue::from_str(&rate_limit::WINDOW_SECS.to_string()) {
        headers.insert("x-ratelimit-window-seconds", v);
    }
}

async fn admin_ratelimit_stats(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<serde_json::Value>, ConvertError> {
    auth::require_admin(&state, &jar).await?;
    Ok(Json(json!({
        "ok": true,
        "ratelimit": state.rate_limiter.stats(),
    })))
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

// ---------- PDF toolkit (premium) ----------

#[derive(Debug, Deserialize)]
struct PdfMergeRequest {
    /// Base64-encoded PDFs to concatenate. Order is preserved.
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PdfSplitRequest {
    pdf_base64: String,
    /// 1-indexed page range, e.g. `"1-3,7,9-z"` (`z` = last).
    pages: String,
}

#[derive(Debug, Deserialize)]
struct PdfCompressRequest {
    pdf_base64: String,
    /// `screen | ebook | printer | prepress`. Defaults to `ebook`.
    #[serde(default)]
    level: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PdfWatermarkRequest {
    pdf_base64: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct PdfEncryptRequest {
    pdf_base64: String,
    user_password: String,
    #[serde(default)]
    owner_password: Option<String>,
}

fn decode_pdf_b64(s: &str) -> Result<Vec<u8>, ConvertError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD
        .decode(s.trim())
        .map_err(|e| ConvertError::BadRequest(format!("invalid base64: {e}")))
}

fn pdf_response(bytes: &[u8]) -> Json<serde_json::Value> {
    let encoded = crate::pdf::encode_base64(bytes);
    Json(json!({
        "ok": true,
        "pdf_base64": encoded,
        "size_bytes": bytes.len(),
    }))
}

async fn pdf_merge_handler(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(payload): Json<PdfMergeRequest>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    usage::reserve(&state.pool, &ctx.user, ctx.api_key_id, "pdf_tools", 1).await?;
    let inputs: Vec<Vec<u8>> = payload
        .files
        .iter()
        .map(|s| decode_pdf_b64(s))
        .collect::<Result<Vec<_>, _>>()?;
    let bytes = pdf_tools::merge(&inputs).await?;
    Ok(pdf_response(&bytes))
}

async fn pdf_split_handler(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(payload): Json<PdfSplitRequest>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    usage::reserve(&state.pool, &ctx.user, ctx.api_key_id, "pdf_tools", 1).await?;
    let input = decode_pdf_b64(&payload.pdf_base64)?;
    let bytes = pdf_tools::split(&input, &payload.pages).await?;
    Ok(pdf_response(&bytes))
}

async fn pdf_compress_handler(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(payload): Json<PdfCompressRequest>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    usage::reserve(&state.pool, &ctx.user, ctx.api_key_id, "pdf_tools", 1).await?;
    let input = decode_pdf_b64(&payload.pdf_base64)?;
    let level = pdf_tools::CompressLevel::from_str(payload.level.as_deref().unwrap_or(""))?;
    let bytes = pdf_tools::compress(&input, level).await?;
    Ok(pdf_response(&bytes))
}

async fn pdf_watermark_handler(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(payload): Json<PdfWatermarkRequest>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    usage::reserve(&state.pool, &ctx.user, ctx.api_key_id, "pdf_tools", 1).await?;
    let input = decode_pdf_b64(&payload.pdf_base64)?;
    // Watermarking renders an overlay PDF via Chromium; grab a slot
    // exactly like a normal PDF conversion would.
    let slot = state.chromium_pool.acquire().await?;
    let bytes =
        pdf_tools::watermark(&state.chromium_bin, Some(slot.data_dir()), &input, &payload.text)
            .await?;
    Ok(pdf_response(&bytes))
}

async fn pdf_encrypt_handler(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(payload): Json<PdfEncryptRequest>,
) -> Result<Json<serde_json::Value>, ConvertError> {
    let ctx = auth::require(&state, &jar, authorization_header(&headers)).await?;
    require_premium(&ctx.user)?;
    usage::reserve(&state.pool, &ctx.user, ctx.api_key_id, "pdf_tools", 1).await?;
    let input = decode_pdf_b64(&payload.pdf_base64)?;
    let bytes = pdf_tools::encrypt(
        &input,
        &payload.user_password,
        payload.owner_password.as_deref(),
    )
    .await?;
    Ok(pdf_response(&bytes))
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
