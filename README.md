# Universal Document Converter

A SaaS monorepo that converts Markdown, HTML, JSON, and XML into normalized
HTML and PDF. The frontend is Astro on Cloudflare Pages; the backend is a Rust
crate compiled to WebAssembly and deployed as a Cloudflare Worker, with
authentication and saved documents stored in Cloudflare D1.

```
/
├── astro-app/           Astro 5 + React islands + Tailwind, Cloudflare adapter
├── rust-backend/        Rust crate → Wasm Worker (worker crate)
│   ├── migrations/      D1 schema
│   └── src/             auth, documents, admin, convert, pdf, crypto, db
├── wrangler.toml        Root config (routes /api/* → worker, rest → Pages)
└── package.json         npm workspaces + dev scripts
```

## What's in the box

- **Anonymous landing page editor** — paste Markdown, render to PDF, download. No
  account required.
- **Free accounts** — save your converted documents to a personal dashboard.
- **Premium accounts** — unlock HTML, JSON, and XML inputs, plus 4 MB payloads
  (vs. 256 KB free).
- **Admin panel** — promote any user to `premium` or `admin` from the web UI.
  Subscriptions can be wired in later; the role column already drives gating.

| Tier      | Inputs                  | Output      | Max payload | Save? |
|-----------|-------------------------|-------------|-------------|-------|
| Anonymous | Markdown                | HTML, PDF   | 256 KB      | No    |
| Free      | Markdown                | HTML, PDF   | 256 KB      | Yes   |
| Premium   | Markdown, HTML, JSON, XML | HTML, PDF | 4 MB        | Yes   |
| Admin     | All of the above + user management                |             |       |

## Prerequisites

- Node.js ≥ 20
- Rust (stable) with the Wasm target:
  `rustup target add wasm32-unknown-unknown`
- `wrangler` CLI (installed as a dev dependency at the root)

## First-time setup

```sh
npm install

# Create a D1 database, then paste the returned database_id into rust-backend/wrangler.toml
wrangler d1 create universal-converter-db

# Apply migrations locally (uses a SQLite file under .wrangler/)
wrangler d1 execute universal-converter-db --local --file ./rust-backend/migrations/0001_initial.sql

# When deploying for real:
wrangler d1 execute universal-converter-db --remote --file ./rust-backend/migrations/0001_initial.sql
```

Set `ADMIN_EMAILS` in `rust-backend/wrangler.toml` to the address(es) that
should be auto-promoted to admin the first time they sign up.

## Local development

```sh
npm run dev
```

Spawns both processes in parallel:

- `npm run dev:astro` — Astro on http://127.0.0.1:4321
- `npm run dev:worker` — `wrangler dev` for the Rust worker on http://127.0.0.1:8787

The Astro dev server proxies `/api/*` to the worker (see
`astro-app/astro.config.mjs`). Cookies are issued without the `Secure` flag in
dev (`COOKIE_SECURE=false`), so sessions work over plain `http://`.

To get an admin account locally:

1. Sign up at http://127.0.0.1:4321/signup using the email listed in
   `ADMIN_EMAILS` (defaults to `admin@example.com`).
2. Visit `/admin` — you can now promote other accounts.

## Build & deploy

```sh
npm run build           # Astro static/SSR bundle + Wasm worker
npm run deploy          # Pages + Worker (requires `wrangler login`)
```

Per-side commands:

```sh
npm --workspace astro-app run deploy
cd rust-backend && wrangler deploy
```

For production, also run the migration against the remote database:

```sh
wrangler d1 execute universal-converter-db --remote --file ./rust-backend/migrations/0001_initial.sql
```

And set the PDF renderer secret:

```sh
wrangler secret put PDF_RENDER_TOKEN
```

## API reference

All endpoints live under `/api` and accept/return JSON. Auth is via an
HttpOnly cookie issued by signup/login.

### Auth
- `POST /api/auth/signup` `{ email, password }` → user + sets cookie
- `POST /api/auth/login` `{ email, password }` → user + sets cookie
- `POST /api/auth/logout` → clears cookie
- `GET  /api/auth/me` → `{ user: PublicUser | null }`

### Conversion
- `POST /api/convert` `{ type, output, content, title? }`
  - `type`: `markdown | html | json | xml` (HTML/JSON/XML require premium)
  - `output`: `html | pdf`
  - Returns `content` (HTML output) or `pdf_base64` (PDF output)
  - Anonymous OK for Markdown → HTML/PDF

### Documents (auth required)
- `GET    /api/documents` — list your saved documents
- `POST   /api/documents` — save `{ title, type, output, content, rendered_html? }`
- `GET    /api/documents/:id` — fetch one (only your own)
- `DELETE /api/documents/:id` — delete one

### Admin (admin role required)
- `GET  /api/admin/users` — list all users
- `POST /api/admin/users/:id/role` `{ role: "free" | "premium" | "admin" }`

## Architecture notes

- **Sessions** are random 32-byte hex tokens stored in D1; cookies are
  HttpOnly + SameSite=Lax (+ Secure in production). Tokens expire after 30 days.
- **Passwords** are hashed with PBKDF2-HMAC-SHA256 (10k iterations) using a
  per-user 16-byte salt. Constant-time comparison via the `subtle` crate.
- **PDF rendering** dispatches an HTTPS request from the worker to an external
  headless-browser service (`PDF_RENDER_URL`), because Chromium cannot run
  inside a Workers isolate.
- **Content collections** in Astro (`/blog`, `/legal`) are prerendered to
  static HTML during build.

## Tests

```sh
cd rust-backend && cargo test
```
