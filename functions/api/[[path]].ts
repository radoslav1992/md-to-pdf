/**
 * Cloudflare Pages Function — reverse-proxy for the JSON API.
 *
 * The site is a static Astro build hosted on Cloudflare Pages, but the API is
 * a Rust (axum) service that runs elsewhere (a VM, Fly.io, Railway, your own
 * box…). This catch-all forwards every `/api/*` request to that backend so the
 * browser still talks to a single first-party origin — which means the session
 * cookie stays first-party and `credentials: 'same-origin'` keeps working with
 * no CORS dance.
 *
 * Configure the backend origin with the `API_ORIGIN` environment variable in
 * the Pages project settings, e.g. `https://api.your-domain.com`. If it is not
 * set the function returns 503 so the misconfiguration is obvious rather than
 * silently serving the SPA shell for API calls.
 */

interface Env {
  API_ORIGIN?: string;
}

// Hop-by-hop headers must not be forwarded across a proxy boundary.
const HOP_BY_HOP = new Set([
  'connection',
  'keep-alive',
  'proxy-authenticate',
  'proxy-authorization',
  'te',
  'trailer',
  'transfer-encoding',
  'upgrade',
]);

export const onRequest: PagesFunction<Env> = async (context) => {
  const { request, env } = context;
  const origin = (env.API_ORIGIN ?? '').replace(/\/$/, '');

  if (!origin) {
    return new Response(
      JSON.stringify({
        ok: false,
        error:
          'API_ORIGIN is not configured for this Cloudflare Pages deployment.',
      }),
      { status: 503, headers: { 'content-type': 'application/json' } },
    );
  }

  const incoming = new URL(request.url);
  const target = new URL(origin);
  // Preserve the full `/api/...` path and query string.
  target.pathname = incoming.pathname;
  target.search = incoming.search;

  // Clone request headers, dropping hop-by-hop and rewriting Host so the
  // upstream sees its own hostname.
  const headers = new Headers(request.headers);
  for (const h of HOP_BY_HOP) headers.delete(h);
  headers.set('host', target.host);
  // Surface the real client IP and original host to the backend.
  const clientIp =
    request.headers.get('cf-connecting-ip') ??
    request.headers.get('x-forwarded-for');
  if (clientIp) {
    headers.set('x-forwarded-for', clientIp);
    headers.set('x-real-ip', clientIp);
  }
  headers.set('x-forwarded-host', incoming.host);
  headers.set('x-forwarded-proto', incoming.protocol.replace(':', ''));

  const init: RequestInit = {
    method: request.method,
    headers,
    redirect: 'manual',
  };
  if (request.method !== 'GET' && request.method !== 'HEAD') {
    init.body = request.body;
  }

  const upstream = await fetch(target.toString(), init);

  // Strip hop-by-hop headers from the response too.
  const respHeaders = new Headers(upstream.headers);
  for (const h of HOP_BY_HOP) respHeaders.delete(h);

  return new Response(upstream.body, {
    status: upstream.status,
    statusText: upstream.statusText,
    headers: respHeaders,
  });
};
