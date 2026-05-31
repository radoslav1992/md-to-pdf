/// <reference path="../.astro/types.d.ts" />
/// <reference types="astro/client" />

interface ImportMetaEnv {
  /**
   * Optional absolute origin for the JSON API (e.g. `https://api.example.com`).
   * Leave unset to use same-origin `/api/*` (Caddy/Docker deploy or the
   * Cloudflare Pages proxy function). See `src/lib/api.ts`.
   */
  readonly PUBLIC_API_BASE?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
