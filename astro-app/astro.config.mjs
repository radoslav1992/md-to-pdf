import { defineConfig } from 'astro/config';
import react from '@astrojs/react';
import tailwind from '@astrojs/tailwind';

// Static site — built into ./dist and served by Caddy alongside the Rust API.
// All dynamic data is fetched client-side from /api/*, so no SSR is needed.
export default defineConfig({
  output: 'static',
  integrations: [
    react(),
    tailwind({
      applyBaseStyles: true,
    }),
  ],
  vite: {
    server: {
      // Dev-time proxy: forwards /api/* to the local axum server on :8000.
      proxy: {
        '/api': {
          target: 'http://127.0.0.1:8000',
          changeOrigin: true,
        },
      },
    },
  },
  server: {
    host: '127.0.0.1',
    port: 4321,
  },
});
