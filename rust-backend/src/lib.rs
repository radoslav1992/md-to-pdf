use worker::*;

mod admin;
mod auth;
mod convert;
mod crypto;
mod db;
mod documents;
mod error;
mod pdf;

use convert::{ConvertRequest, ConvertResponse};

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let router = Router::new();

    router
        .get("/api/health", |_req, _ctx| {
            Response::from_json(&serde_json::json!({
                "ok": true,
                "service": "universal-document-converter-api",
                "version": env!("CARGO_PKG_VERSION"),
            }))
        })
        // Auth
        .post_async("/api/auth/signup", handle_signup)
        .post_async("/api/auth/login", handle_login)
        .post_async("/api/auth/logout", handle_logout)
        .get_async("/api/auth/me", handle_me)
        // Conversion
        .post_async("/api/convert", handle_convert)
        .options("/api/convert", |_req, _ctx| cors_preflight())
        // Documents
        .get_async("/api/documents", handle_documents_list)
        .post_async("/api/documents", handle_documents_create)
        .get_async("/api/documents/:id", handle_documents_get)
        .delete_async("/api/documents/:id", handle_documents_delete)
        // Admin
        .get_async("/api/admin/users", handle_admin_users)
        .post_async("/api/admin/users/:id/role", handle_admin_update_role)
        // Fallback
        .or_else_any_method("/api/*", |_req, _ctx| {
            Response::from_json(&serde_json::json!({
                "ok": false,
                "error": "not_found",
            }))
            .map(|r| r.with_status(404))
        })
        .run(req, env)
        .await
        .map(with_cors)
}

// ---------- Conversion ----------

async fn handle_convert(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: ConvertRequest = match req.json().await {
        Ok(b) => b,
        Err(e) => return error::bad_request(&format!("invalid JSON body: {e}")),
    };

    let env = ctx.env;
    let user = match auth::current_user(&env, &req).await {
        Ok(u) => u,
        Err(e) => return error::from_convert_error(e),
    };
    let response = match convert::run(&body, &env, user.as_ref()).await {
        Ok(r) => r,
        Err(e) => return error::from_convert_error(e),
    };

    let payload: ConvertResponse = response;
    let mut res = Response::from_json(&payload)?;
    res.headers_mut().set("cache-control", "no-store")?;
    Ok(res)
}

// ---------- Auth ----------

async fn handle_signup(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let creds: auth::Credentials = match req.json().await {
        Ok(c) => c,
        Err(e) => return error::bad_request(&format!("invalid JSON: {e}")),
    };
    match auth::signup(&ctx.env, &creds).await {
        Ok((user, token)) => session_response(&ctx.env, &user, &token),
        Err(e) => error::from_convert_error(e),
    }
}

async fn handle_login(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let creds: auth::Credentials = match req.json().await {
        Ok(c) => c,
        Err(e) => return error::bad_request(&format!("invalid JSON: {e}")),
    };
    match auth::login(&ctx.env, &creds).await {
        Ok((user, token)) => session_response(&ctx.env, &user, &token),
        Err(e) => error::from_convert_error(e),
    }
}

async fn handle_logout(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Some(token) = session_token(&req) {
        let _ = auth::logout(&ctx.env, &token).await;
    }
    let mut res = Response::from_json(&serde_json::json!({ "ok": true }))?;
    if let Err(e) = auth::attach_cookie(res.headers_mut(), &auth::build_clear_cookie(&ctx.env)) {
        return error::from_convert_error(e);
    }
    Ok(res)
}

async fn handle_me(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    match auth::current_user(&ctx.env, &req).await {
        Ok(Some(user)) => Response::from_json(&serde_json::json!({
            "ok": true,
            "user": user.public(),
        })),
        Ok(None) => Response::from_json(&serde_json::json!({
            "ok": true,
            "user": null,
        })),
        Err(e) => error::from_convert_error(e),
    }
}

fn session_response(env: &Env, user: &db::User, token: &str) -> Result<Response> {
    let mut res = Response::from_json(&serde_json::json!({
        "ok": true,
        "user": user.public(),
    }))?;
    if let Err(e) = auth::attach_cookie(res.headers_mut(), &auth::build_session_cookie(env, token))
    {
        return error::from_convert_error(e);
    }
    Ok(res)
}

fn session_token(req: &Request) -> Option<String> {
    let header = req.headers().get("cookie").ok().flatten()?;
    for part in header.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k == "session" && !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

// ---------- Documents ----------

async fn handle_documents_list(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match auth::require_user(&ctx.env, &req).await {
        Ok(u) => u,
        Err(e) => return error::from_convert_error(e),
    };
    match documents::list(&ctx.env, &user).await {
        Ok(items) => Response::from_json(&serde_json::json!({
            "ok": true,
            "items": items,
        })),
        Err(e) => error::from_convert_error(e),
    }
}

async fn handle_documents_create(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match auth::require_user(&ctx.env, &req).await {
        Ok(u) => u,
        Err(e) => return error::from_convert_error(e),
    };
    let payload: documents::SaveDocument = match req.json().await {
        Ok(p) => p,
        Err(e) => return error::bad_request(&format!("invalid JSON: {e}")),
    };
    // Non-premium users may only save documents created from Markdown input.
    if !user.is_premium() && !matches!(payload.input_type.to_ascii_lowercase().as_str(), "markdown" | "md") {
        return error::from_convert_error(error::ConvertError::PremiumRequired);
    }
    match documents::save(&ctx.env, &user, &payload).await {
        Ok(doc) => Response::from_json(&serde_json::json!({
            "ok": true,
            "document": doc,
        })),
        Err(e) => error::from_convert_error(e),
    }
}

async fn handle_documents_get(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match auth::require_user(&ctx.env, &req).await {
        Ok(u) => u,
        Err(e) => return error::from_convert_error(e),
    };
    let id = match parse_id_param(&ctx, "id") {
        Some(id) => id,
        None => return error::bad_request("missing or invalid id"),
    };
    match documents::get(&ctx.env, &user, id).await {
        Ok(doc) => Response::from_json(&serde_json::json!({
            "ok": true,
            "document": doc,
        })),
        Err(e) => error::from_convert_error(e),
    }
}

async fn handle_documents_delete(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match auth::require_user(&ctx.env, &req).await {
        Ok(u) => u,
        Err(e) => return error::from_convert_error(e),
    };
    let id = match parse_id_param(&ctx, "id") {
        Some(id) => id,
        None => return error::bad_request("missing or invalid id"),
    };
    match documents::delete(&ctx.env, &user, id).await {
        Ok(()) => Response::from_json(&serde_json::json!({ "ok": true })),
        Err(e) => error::from_convert_error(e),
    }
}

// ---------- Admin ----------

async fn handle_admin_users(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(e) = auth::require_admin(&ctx.env, &req).await {
        return error::from_convert_error(e);
    }
    match admin::list_users(&ctx.env).await {
        Ok(items) => Response::from_json(&serde_json::json!({
            "ok": true,
            "items": items,
        })),
        Err(e) => error::from_convert_error(e),
    }
}

async fn handle_admin_update_role(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let actor = match auth::require_admin(&ctx.env, &req).await {
        Ok(u) => u,
        Err(e) => return error::from_convert_error(e),
    };
    let id = match parse_id_param(&ctx, "id") {
        Some(id) => id,
        None => return error::bad_request("missing or invalid id"),
    };
    let payload: admin::UpdateRole = match req.json().await {
        Ok(p) => p,
        Err(e) => return error::bad_request(&format!("invalid JSON: {e}")),
    };
    match admin::update_role(&ctx.env, &actor, id, &payload).await {
        Ok(user) => Response::from_json(&serde_json::json!({
            "ok": true,
            "user": user,
        })),
        Err(e) => error::from_convert_error(e),
    }
}

fn parse_id_param(ctx: &RouteContext<()>, name: &str) -> Option<i64> {
    ctx.param(name).and_then(|v| v.parse::<i64>().ok())
}

// ---------- CORS ----------

fn cors_preflight() -> Result<Response> {
    let mut res = Response::empty()?;
    let headers = res.headers_mut();
    headers.set("access-control-allow-origin", "*")?;
    headers.set("access-control-allow-methods", "GET, POST, DELETE, OPTIONS")?;
    headers.set("access-control-allow-headers", "content-type, authorization")?;
    headers.set("access-control-allow-credentials", "true")?;
    headers.set("access-control-max-age", "86400")?;
    Ok(res.with_status(204))
}

fn with_cors(mut res: Response) -> Response {
    let _ = res.headers_mut().set("access-control-allow-origin", "*");
    let _ = res
        .headers_mut()
        .set("access-control-allow-credentials", "true");
    res
}
