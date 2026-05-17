use worker::*;

mod convert;
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
        .post_async("/api/convert", handle_convert)
        .options("/api/convert", |_req, _ctx| cors_preflight())
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

async fn handle_convert(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: ConvertRequest = match req.json().await {
        Ok(b) => b,
        Err(e) => return error::bad_request(&format!("invalid JSON body: {e}")),
    };

    let env = ctx.env;
    let response = match convert::run(&body, &env).await {
        Ok(r) => r,
        Err(e) => return error::from_convert_error(e),
    };

    let payload: ConvertResponse = response;
    let mut res = Response::from_json(&payload)?;
    res.headers_mut().set("cache-control", "no-store")?;
    Ok(res)
}

fn cors_preflight() -> Result<Response> {
    let mut res = Response::empty()?;
    let headers = res.headers_mut();
    headers.set("access-control-allow-origin", "*")?;
    headers.set("access-control-allow-methods", "GET, POST, OPTIONS")?;
    headers.set("access-control-allow-headers", "content-type, authorization")?;
    headers.set("access-control-max-age", "86400")?;
    Ok(res.with_status(204))
}

fn with_cors(mut res: Response) -> Response {
    let _ = res.headers_mut().set("access-control-allow-origin", "*");
    res
}
