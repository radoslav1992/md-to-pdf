# Universal Document Converter

A self-hosted SaaS that converts Markdown, HTML, JSON, and XML into HTML and
PDF. Frontend is Astro built to static HTML; backend is a Rust axum service
with SQLite for storage and a local headless Chromium for PDF rendering.
Everything runs in Docker Compose behind Caddy.

```
/
├── astro-app/           Astro 5 + React islands + Tailwind  (built to static HTML)
├── rust-backend/        Rust + axum + sqlx (sqlite)
│   ├── migrations/      SQLite schema
│   ├── src/             auth, documents, admin, convert, pdf, crypto, db
│   └── Dockerfile       API image (Rust binary + chromium)
├── Caddyfile            Reverse proxy: /api/* → api, /* → static
├── Dockerfile.web       Multi-stage: builds Astro, bakes into Caddy image
├── docker-compose.yml   2 services: api + web (caddy)
└── .env.example         Configuration template
```

## Feature matrix

| Tier      | Inputs                                                    | Output    | Max payload | Quota / mo | Save? | Premium-only |
|-----------|-----------------------------------------------------------|-----------|-------------|------------|-------|--------------|
| Anonymous | Markdown                                                  | HTML, PDF | 256 KB      | unmetered  | No    | —            |
| Free      | Markdown                                                  | HTML, PDF | 256 KB      | 100        | Yes   | —            |
| Premium   | + HTML, JSON, XML, CSV, Org-mode, AsciiDoc, RST, LaTeX    | HTML, PDF, DOCX, EPUB, ODT, PNG, JPG, Markdown | 16 MB       | 10,000     | Yes   | 6 themes, custom CSS, templates, PDF page setup, cover/header/footer, page numbers, PDF/A archival, multi-file Markdown, table of contents, syntax highlighting, LaTeX math, Mermaid diagrams, PDF toolkit (merge / split / compress / watermark / encrypt), API keys, batch conversion, signed webhooks, background jobs, version history + restore, shareable links, PDF→Markdown (OCR), AES-256-GCM at rest |
| Admin     | All of the above + user role management + unlimited quota                                                                                                            |

Subscriptions aren't built yet — for now an admin promotes accounts to
`premium` manually from `/admin`.

The editor offers live preview (debounced auto-render on edits) and
drag-and-drop image upload for any signed-in user; both work without a
premium account.

## Quick start on the Hetzner box (167.235.146.183)

Assuming a fresh Ubuntu 24.04 install on the CX23:

```sh
# 1. Install Docker
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER     # log out and back in afterwards

# 2. Clone the repo
git clone <repo-url> /opt/converter
cd /opt/converter

# 3. Configure
cp .env.example .env
$EDITOR .env                       # set ADMIN_EMAILS to your address

# 4. Open the firewall on port 80
sudo ufw allow 80/tcp

# 5. Bring everything up
docker compose up -d --build
```

You should now be able to hit `http://167.235.146.183/` in a browser. First
person to sign up with an email listed in `ADMIN_EMAILS` is auto-promoted to
`admin` and can promote other accounts at `/admin`.

To watch logs:

```sh
docker compose logs -f api
docker compose logs -f web
```

To update after a `git pull`:

```sh
docker compose up -d --build
```

The SQLite database lives in a named volume (`universal-converter_api_data`)
so rebuilds don't wipe your users.

### Adding TLS later

When you point a domain at the IP, change `:80` at the top of `Caddyfile` to
your domain name and set `COOKIE_SECURE=true` in `.env`. Caddy will provision
Let's Encrypt automatically on next restart. Open port 443 in ufw.

## Local development (without Docker)

You need the Rust toolchain, Node ≥ 20, and Chromium installed locally
(`apt install chromium-browser` / `brew install --cask chromium`).

```sh
npm install
npm run dev
```

This runs two processes in parallel:

- Astro dev server on http://127.0.0.1:4321
- Rust API on http://127.0.0.1:8000 (SQLite at `rust-backend/dev.db`)

Astro proxies `/api/*` to the API server (see `astro-app/astro.config.mjs`),
so the UI works against your local backend with no extra config.

## How PDF rendering works

Headless Chromium runs *inside the API container*. When a request hits
`POST /api/convert` with `output: "pdf"`, the worker:

1. Normalizes the input format to HTML (markdown / html / json / xml / csv /
   org natively; asciidoc / rst / latex via `pandoc` shelled out from the
   same container)
2. Wraps the body with the chosen theme CSS plus `@page` rules built from
   `pdf_options`
3. Writes the HTML to a temp file
4. Spawns `chromium --headless --print-to-pdf=/tmp/x.pdf file:///tmp/x.html`
   (with `--no-pdf-header-footer` unless `pdf_options.page_numbers` is set)
5. Reads the resulting PDF and base64-encodes it in the JSON response

Custom headers, footers, and cover pages are rendered as `position: fixed`
elements and `page-break-after` sections in the document itself, so they
work on every Chromium version without needing the DevTools protocol.

No third-party service. Chromium + pandoc are bundled in the API image
(~450 MB). The container has a 2 GB memory limit; each render uses
~150–300 MB. Concurrent PDF renders are bounded by a semaphore (default 5).

## API reference

All endpoints accept/return JSON. Auth is via an HttpOnly session cookie
**or** an `Authorization: Bearer <api-key>` header.

### Versioning

Every endpoint is reachable at two paths:

- `/api/v1/<endpoint>` — canonical, stable v1 surface
- `/api/<endpoint>`    — legacy unversioned alias (kept forever)

Future breaking changes will land at `/api/v2/...` without disturbing
v1. New clients should always prefer the explicit version.

### Ops

- `GET /api/v1/healthz` — pure liveness probe; always 200.
- `GET /api/v1/readyz`  — readiness probe; pings the DB and checks the
  Chromium binary. Returns 503 when the DB is unreachable. Chromium
  absence is reported as a soft warning, not a hard fail.
- `GET /api/v1/metrics` — Prometheus text-format scrape: request /
  response counters, a `udc_request_duration_seconds` histogram, and
  gauges for DB pool size, active URL watches, queued/running jobs,
  rate-limit buckets, and uptime.
- `GET /api/v1/admin/backup` — streams a consistent SQLite snapshot
  via `VACUUM INTO` (works while the API is live, because the DB is
  in WAL mode). Filename includes a UTC timestamp. Available in the
  admin UI as a "Download backup" button. **Restore is an operator
  action**: stop the containers, drop the file into
  `/data/converter.db`, restart.
- `GET /api/v1/jobs/{id}/events` — Server-Sent Events stream for one
  job. Emits the initial state, one event per status change, and
  closes on `done/failed/canceled` (or after 30 min, whichever comes
  first). The editor's Jobs panel subscribes automatically when a
  running job is selected so its status updates in real time.

These three paths plus `/health`, `/openapi.yaml`, and `/openapi.json`
bypass the rate limiter so monitors and Prometheus scrapes never get
throttled.

Every response carries an `X-Request-Id` header. Inbound
`X-Request-Id` values (set by a reverse proxy) are honoured if
present, capped at 128 chars; otherwise the server generates a v4
UUID. Set `RUST_LOG_FORMAT=json` to get one-line JSON log records
suitable for Loki / Grafana / CloudWatch / Datadog ingestion.

### OpenAPI & Docs

- `GET /api/v1/openapi.yaml` — the hand-written OpenAPI 3.1 spec
- `GET /api/v1/openapi.json` — same spec, JSON encoding
- `GET /api-docs` — interactive Redoc-rendered reference (vendored JS,
  no third-party calls)

To generate SDKs from the spec, point any OpenAPI generator at it. For
example, the official [openapi-generator-cli](https://openapi-generator.tech):

```sh
# TypeScript axios client
openapi-generator-cli generate -i http://localhost:8000/api/v1/openapi.yaml \
  -g typescript-axios -o ./sdk-ts

# Python pydantic + requests
openapi-generator-cli generate -i http://localhost:8000/api/v1/openapi.yaml \
  -g python -o ./sdk-py
```

### Rate limiting

Every request after auth resolution is metered against a per-identity
token bucket. The bucket key is, in order:

1. API key id  (when `Authorization: Bearer …` is used)
2. User id     (when the session cookie is used)
3. IP          (anonymous — uses `X-Forwarded-For` first hop or the peer addr)

Capacities per tier:

| Tier      | Burst | Refill    |
|-----------|-------|-----------|
| Anonymous |    30 |  30 / min |
| Free      |    60 |  60 / min |
| Premium   |   600 | 600 / min |
| Admin     | 6 000 | 6 000/min |

Responses carry `X-RateLimit-Limit`, `X-RateLimit-Remaining`, and
`X-RateLimit-Window-Seconds`. When the bucket is empty the API returns
`429 Too Many Requests` with a `Retry-After` header. `/health` and
`/openapi.{yaml,json}` bypass the limiter so monitors and dashboards
never get throttled.

### Auth
- `POST /api/auth/signup` `{ email, password }` → user + sets cookie
- `POST /api/auth/login`  `{ email, password }` → user + sets cookie
- `POST /api/auth/logout` → clears cookie
- `GET  /api/auth/me` → `{ user: PublicUser | null }`

### Conversion
- `POST /api/convert` `{ type, output, content, title?, theme?, custom_css?, pdf_options?, files?, template_id?, enrichments? }`
  - `type`: `markdown | html | json | xml | csv | org | asciidoc | rst | latex`
    (only `markdown` is available without premium)
  - `output`: `html | pdf | markdown | docx | epub | odt | png | jpg`
    - **html** — returns the rendered document in `content`
    - **pdf** — Chromium-rendered, returned in both `pdf_base64`
      (legacy field) and `output_base64`
    - **markdown** — pipes the rendered HTML through pandoc → GFM
      Markdown; useful when you want enrichments (TOC, syntax-
      highlighted code) baked into a Markdown export
    - **docx / epub / odt** — pandoc-driven binary archives, returned
      in `output_base64`
    - **png / jpg** — single-page Chromium screenshot of the rendered
      document at 1280 × 1600, returned in `output_base64`
    - All binary outputs also set `output_mime` so clients can pick the
      right download filename without hard-coding cases.
  - `theme`: `default` (free) or `clean | academic | resume | letter | github` (premium)
  - `custom_css`: arbitrary CSS string appended to the document (premium)
  - `pdf_options` (premium, applies when `output: "pdf"`):
    - `page_size`: `A4 | A3 | A5 | Letter | Legal | Tabloid` (default `A4`)
    - `orientation`: `portrait | landscape`
    - `margin`: `{ top, right, bottom, left }` — CSS lengths like `"1in"`, `"20mm"`
    - `page_numbers`: `bool` — Chromium's built-in footer with page numbers + title
    - `header_template` / `footer_template`: HTML that repeats on every page
    - `cover`: `{ title, subtitle, author, date }` — prepended cover page
  - `files`: `[ { path, content }, … ]` — multi-file bundle, concatenated in
    array order (premium; text formats only)
  - `template_id` (premium): expand the named template's theme / custom_css /
    pdf_options as defaults — request-level fields still override
  - `enrichments` (premium): `{ toc?: bool, toc_depth?: 1..4, syntax_highlight?: bool, math?: bool, mermaid?: bool }`
    — auto-table of contents, server-side syntect highlighting, LaTeX
    math rendered to MathML, and Mermaid diagrams. The Mermaid bundle is
    vendored at `rust-backend/vendor/mermaid.min.js` and inlined into the
    rendered document; Chromium runs it during PDF capture so no
    third-party network call is required.
  - Identical request/response shape regardless of `output`; just pick
    the right field for the format (`content` for text outputs,
    `output_base64` for binary ones).
  - Responses set `cached: true` when served from the in-memory render
    cache (keyed by every input that can change the output, including
    the calling user so per-user image inlining stays sound).
  - Anonymous OK for Markdown with default theme and no premium features

### Batch conversion (premium)
- `POST /api/convert/batch?format=json|zip` `{ items: [ConvertRequest…], webhook? }`
  - Up to 50 items per batch. Each item counts as 1 against your quota.
  - `format=zip` returns a zip archive (`000.pdf`, `001.html`, `002.error.txt`, …);
    `format=json` (default) returns a JSON array of per-item results.
  - `webhook`: `{ url, secret? }` — when present, the JSON response is also
    POSTed to `url`. If `secret` is set, the body is HMAC-SHA-256 signed
    and the digest is sent in the `X-UDC-Signature` header.

### URL watches (premium)
Inverse of batch webhooks: a background worker polls a URL on a
schedule, and whenever the content changes it runs a conversion and
POSTs the result to your `target_url`.
- `GET    /api/v1/url-watches`        — list your watches
- `POST   /api/v1/url-watches`        — create
- `GET    /api/v1/url-watches/:id`    — fetch one
- `PATCH  /api/v1/url-watches/:id`    — update
- `DELETE /api/v1/url-watches/:id`    — delete

Body fields: `name`, `url` (http(s) to poll), `input_type`,
`output_format` (`html | pdf`), `target_url`, optional `target_secret`,
`poll_interval_secs` (60..=604800; default 900). When `target_secret`
is set the JSON delivery body is HMAC-SHA-256 signed and the digest
is sent in `X-UDC-Signature`. The poller wakes every 30 s and processes
up to 32 due watches per tick.

### Templates (premium)
- `GET    /api/templates`        — list your templates
- `POST   /api/templates`        — create `{ name, theme?, custom_css?, pdf_options? }`
- `GET    /api/templates/:id`    — fetch one
- `PATCH  /api/templates/:id`    — update
- `DELETE /api/templates/:id`    — delete

### API keys (premium)
- `GET    /api/keys`             — list (hashes only; the plaintext is never returned again)
- `POST   /api/keys` `{ name }`  — create; returns `{ key, plaintext }`
- `DELETE /api/keys/:id`         — revoke

### Usage
- `GET /api/usage` → `{ used, limit, period_start }` — rolling 30-day window

### Async jobs (premium)
- `POST /api/jobs/convert` — same body as `/api/convert`, returns `{ job_id, status }`
- `POST /api/jobs/batch`   — same body as `/api/convert/batch`, returns `{ job_id, status }`
- `GET  /api/jobs`         — list your jobs (newest first, up to 100)
- `GET  /api/jobs/:id`     — poll for status/result; `status` cycles
  `queued → running → done` (or `failed` / `canceled`)
- `POST /api/jobs/:id/cancel` — cancel a queued job

### Document versions
- `GET  /api/documents/:id/versions`        — list (up to 50, oldest pruned)
- `GET  /api/documents/:id/versions/:vid`   — fetch full content of one version
- `POST /api/documents/:id/versions/:vid/restore` — apply a version onto the
  document, snapshotting the current state first

### Encrypted documents (premium)
Pass `encrypt_password: "..."` (≥8 chars) when saving or updating a document.
The body is AES-256-GCM encrypted under a key derived from the password
(PBKDF2-HMAC-SHA-256, 200k iterations). The server discards the password
after key derivation. Encrypted documents cannot be shared via public links.
- `POST /api/documents/:id/decrypt` `{ password }` → `{ content }`

### Shareable links (premium)
- `POST /api/documents/:id/shares` `{ format, expires_in_seconds?, password? }`
  → `{ share, url, token }` — **the token is shown once**
- `GET  /api/documents/:id/shares` — list active shares for a doc
- `DELETE /api/shares/:id` — revoke
- `GET  /api/share/:token` — public, returns `{ format, requires_password, expires_at }`
- `POST /api/share/:token` `{ password? }` → the rendered document

### PDF → Markdown (premium)
- `POST /api/extract` `{ pdf_base64, ocr? }` → `{ markdown, method, page_count }`
  - `ocr`: `auto` (default — OCRs only when pdftotext is sparse), `force`, `off`
  - PDFs up to 16 MB; backed by `poppler-utils` + `tesseract-ocr-eng`

### PDF toolkit (premium)
Structural PDF manipulation via the bundled `qpdf` and `ghostscript`.
Every endpoint takes the input PDF(s) as base64 and returns the result
the same way (`{ pdf_base64, size_bytes }`); inputs are capped at 64 MiB
each.
- `POST /api/pdf/merge`     `{ files: [base64, …] }` — concatenate up to
  50 PDFs in order
- `POST /api/pdf/split`     `{ pdf_base64, pages }` — extract a 1-indexed
  page range (e.g. `"1-3,7,9-z"`, where `z` means "last")
- `POST /api/pdf/compress`  `{ pdf_base64, level? }` — re-stream through
  ghostscript at `screen | ebook | printer | prepress` (default `ebook`)
- `POST /api/pdf/watermark` `{ pdf_base64, text }` — stamp every page
  with rotated translucent text; uses Chromium to render the overlay
- `POST /api/pdf/encrypt`   `{ pdf_base64, user_password, owner_password? }`
  — AES-256 password protection (when supported by the installed qpdf)

`POST /api/convert` also gained `pdf_options.pdf_a: bool`, which
post-processes the Chromium output through `gs -dPDFA=2` for archival-
grade (ISO 19005-2) output. Adds ~1s per render.

### Images (auth required)
Image storage for documents. The editor's drag-and-drop handler uses
these endpoints; you can also call them directly to embed images into
Markdown via `![alt](/api/images/:id)`. For PDF output the URLs are
rewritten to inline `data:` URIs by the server before Chromium loads the
page, so file://-based renders still see the bytes.
- `GET    /api/images`            — list your uploaded images
- `POST   /api/images` `{ filename, content_type, data_base64 }` → `{ image }`
  - PNG / JPEG / GIF / WebP / SVG, ≤10 MiB per file, ≤256 MiB per user.
    Uploads are deduplicated per user by SHA-256.
- `GET    /api/images/:id`        — serve the bytes (cookie or API key required)
- `DELETE /api/images/:id`        — remove

### Documents (auth required)
- `GET    /api/documents?q=&folder=&tag=` — list your saved documents
  - `q`: full-text search (FTS5 prefix-matched on title + content;
    encrypted documents are matched on title only). Returns `items`,
    plus `folders` and `tags` arrays for sidebar UIs.
  - `folder`: exact folder match. Pass `?folder=` for top-level only.
  - `tag`: single tag, case-insensitive.
- `POST   /api/documents`        — save `{ title, type, output, content, rendered_html?, theme?, custom_css?, pdf_options?, folder?, tags? }`
  - `folder`: free-form path (e.g. `"Work/Drafts"`, normalised to `/`)
  - `tags`: array of short slugs; lowercased and deduplicated
    (≤16 tags, ≤32 chars each)
- `GET    /api/documents/:id`    — fetch one (only your own)
- `PATCH  /api/documents/:id`    — update one (same body as POST)
- `DELETE /api/documents/:id`    — delete one

### Admin (admin role required)
- `GET  /api/admin/users`              — list all users
- `POST /api/admin/users/:id/role`     `{ role: "free" | "premium" | "admin" }`
- `GET  /api/admin/cache`              — render-cache stats
  (entries, total_bytes, max_bytes, hits, misses). Cap configurable via
  `RENDER_CACHE_MAX_BYTES` env (default 128 MiB).
- `GET  /api/admin/ratelimit`          — current rate-limit bucket count
  + cap (the bucket cap is configurable via `RATE_LIMIT_MAX_BUCKETS`)

## CLI

`cli/` is a small standalone Rust binary that wraps the API. It uses
the `/api/v1` surface, supports both env-var and saved-config auth, and
ships in the workspace alongside the API.

```sh
# Build it
cargo build --release --bin udc
./target/release/udc --help

# Persist credentials so you don't have to pass them every time
udc login https://docs.example.com paste-api-key-here   # or `-` to read from stdin

# Convert and write to a file
udc convert README.md -o README.pdf --format pdf

# Pipe through stdin/stdout
cat doc.md | udc convert - -o - --type markdown --format html > doc.html

# Quick status
udc health
udc usage
udc watches
```

Config file lives at the OS-standard config dir (`udc config` prints
the path).

## GitHub Action

A composite action lives at `.github/actions/convert`. Drop it into any
repo workflow alongside the secrets and it will install the `udc` CLI
(via `cargo install`) and run a conversion. See
`.github/workflows/example-convert.yml` for the canonical example.

```yaml
- uses: dtolnay/rust-toolchain@stable
- uses: radoslav1992/md-to-pdf/.github/actions/convert@main
  with:
    base-url: ${{ vars.UDC_BASE_URL }}
    api-key:  ${{ secrets.UDC_API_KEY }}
    input:    README.md
    format:   pdf
```

## Architecture notes

- **axum + sqlx** for the HTTP layer; SQLite in WAL mode handles concurrent
  reads + serialized writes well into the thousands of req/s on this hardware.
- **Sessions** are random 32-byte hex tokens stored in SQLite; cookies are
  HttpOnly + SameSite=Lax (+ Secure once TLS is on). 30-day TTL.
- **Passwords** are hashed with PBKDF2-HMAC-SHA256 (100k iterations) using a
  per-user 16-byte salt. Constant-time comparison via the `subtle` crate.
- **PDF rendering** spawns local Chromium against a pool of pre-created
  `--user-data-dir` slots so each render reuses the previous one's
  font / shader / GPU caches (≈ 30% startup saved on cold paths). Size
  is `CHROMIUM_POOL_SIZE` (default 5). The pool also gates concurrent
  renders the same way the old semaphore did. No outbound network calls.
- **Astro** is built to plain static HTML; React islands fetch from `/api`
  client-side. No SSR, no Node server in production.
- **Caddy** terminates HTTP/HTTPS and reverse-proxies `/api/*` to the API
  container over the internal Docker network.

## Tests

Backend (Rust):

```sh
cd rust-backend && cargo test
# or, from the repo root:
cargo test --workspace
```

End-to-end (Playwright). The config in `playwright.config.ts` spins
up both the Astro dev server and the Rust API, then runs the suite in
`e2e/` against Chromium. Tests cover the homepage, anonymous Markdown
→ HTML conversion via the live API, and the signup flow.

```sh
npm install                         # if you haven't yet
npm run test:e2e:install            # one-time Chromium download
npm run test:e2e                    # full golden-path suite
```

The suite is deliberately small — three golden-path tests that block
regressions on the routes a brand-new visitor is most likely to hit.
Add new specs under `e2e/*.spec.ts`.
