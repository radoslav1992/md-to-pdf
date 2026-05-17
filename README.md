# Universal Document Converter

A monorepo SaaS that converts Markdown, HTML, JSON, and XML into normalized
HTML and PDF. The frontend is Astro on Cloudflare Pages; the backend is a Rust
crate compiled to WebAssembly and deployed as a Cloudflare Worker.

```
/
├── astro-app/        Astro 5 + React islands + Tailwind, Cloudflare adapter
├── rust-backend/     Rust crate → Wasm Worker (worker crate)
├── wrangler.toml     Root config (routes /api/* → worker, rest → Pages)
└── package.json      npm workspaces + dev scripts
```

## Prerequisites

- Node.js ≥ 20
- Rust (stable) with `wasm32-unknown-unknown` target:
  `rustup target add wasm32-unknown-unknown`
- `wrangler` CLI (installed as a dev dependency at the root)

## Setup

```sh
npm install
```

## Local development

Spin up the Astro dev server and the Rust worker in parallel:

```sh
npm run dev
```

This runs two processes:

- `npm run dev:astro` — Astro on http://127.0.0.1:4321
- `npm run dev:worker` — `wrangler dev` for the Rust worker on http://127.0.0.1:8787

The Astro dev server proxies `/api/*` to the worker (see `astro-app/astro.config.mjs`),
so visiting http://127.0.0.1:4321/editor and clicking **Convert** routes the
request through to the Rust backend transparently.

## Build

```sh
npm run build
```

- `build:astro` produces `astro-app/dist/` (deployable to Cloudflare Pages)
- `build:worker` produces a Wasm module via `worker-build` for the worker

## Test

```sh
cd rust-backend && cargo test
```

## Deploy

The root `wrangler.toml` defines the worker routes (it expects you to replace
`your-domain.com` with a zone you own). After running `wrangler login`:

```sh
npm run deploy
```

- `deploy:astro` publishes the static + SSR output to Pages
- `deploy:worker` publishes the Rust worker

You can also deploy each side independently:

```sh
npm --workspace astro-app run deploy
cd rust-backend && wrangler deploy
```

## API

### `POST /api/convert`

Request body:

```json
{
  "type": "markdown" | "html" | "json" | "xml",
  "output": "html" | "pdf",
  "content": "...",
  "title": "Optional document title"
}
```

Response:

```json
{
  "ok": true,
  "output_type": "html",
  "content": "<!doctype html>..."
}
```

For PDF output the response contains `pdf_base64` (base64-encoded PDF bytes).
Headless browsers cannot run inside a Workers isolate, so the worker dispatches
to an external rendering service (Browserless / Puppeteer-compatible) configured
via:

- `PDF_RENDER_URL` (var) — render endpoint, defaults to Browserless
- `PDF_RENDER_TOKEN` (secret) — bearer token, set with `wrangler secret put`

### `GET /api/health`

Returns service metadata for liveness probes.

## Content collections

The Astro app uses content collections for `/blog` and `/legal`:

- `astro-app/src/content/blog/*.md` — blog posts (schema in `config.ts`)
- `astro-app/src/content/legal/*.md` — legal documents

Both are prerendered to static HTML during build.
