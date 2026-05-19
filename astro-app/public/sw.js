// Service worker for the Universal Document Converter.
//
// Strategy:
//   - On install, prime a cache with the shell entry point and favicon so
//     a cold start has something to render.
//   - For navigation (HTML) requests, network-first with a cache fallback
//     so users get the latest version when online but still see the app
//     when offline.
//   - For same-origin GET assets (Astro's hashed /_astro/* bundles, the
//     favicon, the manifest), cache-first. Hashed filenames mean we can
//     keep them forever; the cache is replaced wholesale when the SW
//     version bumps.
//   - Anything else (POSTs, cross-origin, /api/*) bypasses the worker.
//
// To force a rebuild on every deploy, bump CACHE_VERSION.

const CACHE_VERSION = 'udc-v1';
const SHELL_ASSETS = ['/', '/favicon.svg', '/manifest.webmanifest'];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_VERSION).then((cache) => cache.addAll(SHELL_ASSETS).catch(() => {})),
  );
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(
        keys.filter((k) => k !== CACHE_VERSION).map((k) => caches.delete(k)),
      ),
    ),
  );
  self.clients.claim();
});

self.addEventListener('fetch', (event) => {
  const req = event.request;
  if (req.method !== 'GET') return;

  const url = new URL(req.url);
  if (url.origin !== self.location.origin) return;
  // Never get between the editor and its API.
  if (url.pathname.startsWith('/api/')) return;

  // HTML navigations: network-first.
  const accept = req.headers.get('accept') || '';
  if (req.mode === 'navigate' || accept.includes('text/html')) {
    event.respondWith(
      fetch(req)
        .then((res) => {
          const copy = res.clone();
          caches.open(CACHE_VERSION).then((cache) => cache.put(req, copy)).catch(() => {});
          return res;
        })
        .catch(() => caches.match(req).then((cached) => cached || caches.match('/'))),
    );
    return;
  }

  // Static assets: cache-first.
  event.respondWith(
    caches.match(req).then(
      (cached) =>
        cached ||
        fetch(req)
          .then((res) => {
            const copy = res.clone();
            caches.open(CACHE_VERSION).then((cache) => cache.put(req, copy)).catch(() => {});
            return res;
          })
          .catch(() => cached),
    ),
  );
});
