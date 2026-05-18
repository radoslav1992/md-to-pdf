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

| Tier      | Inputs                       | Output    | Max payload | Save? |
|-----------|------------------------------|-----------|-------------|-------|
| Anonymous | Markdown                     | HTML, PDF | 256 KB      | No    |
| Free      | Markdown                     | HTML, PDF | 256 KB      | Yes   |
| Premium   | Markdown, HTML, JSON, XML    | HTML, PDF | 4 MB        | Yes   |
| Admin     | All of the above + user role management         |             |       |

Subscriptions aren't built yet — for now an admin promotes accounts to
`premium` manually from `/admin`.

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

1. Normalizes the input format to HTML
2. Writes the HTML to a temp file
3. Spawns `chromium --headless --print-to-pdf=/tmp/x.pdf file:///tmp/x.html`
4. Reads the resulting PDF and base64-encodes it in the JSON response

No third-party service. Chromium is bundled in the API image (~300 MB).
The container has a 2 GB memory limit; each render uses ~150–300 MB. Concurrent
requests run in parallel but you'll want to add a queue if you expect more
than ~5 simultaneous PDF jobs on a CX23.

## API reference

All endpoints accept/return JSON. Auth is via an HttpOnly session cookie.

### Auth
- `POST /api/auth/signup` `{ email, password }` → user + sets cookie
- `POST /api/auth/login`  `{ email, password }` → user + sets cookie
- `POST /api/auth/logout` → clears cookie
- `GET  /api/auth/me` → `{ user: PublicUser | null }`

### Conversion
- `POST /api/convert` `{ type, output, content, title? }`
  - `type`: `markdown | html | json | xml`  (HTML/JSON/XML require premium)
  - `output`: `html | pdf`
  - Returns `content` (HTML output) or `pdf_base64` (PDF output)
  - Anonymous OK for Markdown

### Documents (auth required)
- `GET    /api/documents`        — list your saved documents
- `POST   /api/documents`        — save `{ title, type, output, content, rendered_html? }`
- `GET    /api/documents/:id`    — fetch one (only your own)
- `DELETE /api/documents/:id`    — delete one

### Admin (admin role required)
- `GET  /api/admin/users`              — list all users
- `POST /api/admin/users/:id/role`     `{ role: "free" | "premium" | "admin" }`

## Architecture notes

- **axum + sqlx** for the HTTP layer; SQLite in WAL mode handles concurrent
  reads + serialized writes well into the thousands of req/s on this hardware.
- **Sessions** are random 32-byte hex tokens stored in SQLite; cookies are
  HttpOnly + SameSite=Lax (+ Secure once TLS is on). 30-day TTL.
- **Passwords** are hashed with PBKDF2-HMAC-SHA256 (100k iterations) using a
  per-user 16-byte salt. Constant-time comparison via the `subtle` crate.
- **PDF rendering** spawns local Chromium. No outbound network calls.
- **Astro** is built to plain static HTML; React islands fetch from `/api`
  client-side. No SSR, no Node server in production.
- **Caddy** terminates HTTP/HTTPS and reverse-proxies `/api/*` to the API
  container over the internal Docker network.

## Tests

```sh
cd rust-backend && cargo test
```
