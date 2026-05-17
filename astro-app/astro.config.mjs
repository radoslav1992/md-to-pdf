import { defineConfig } from 'astro/config';
import cloudflare from '@astrojs/cloudflare';
import react from '@astrojs/react';
import tailwind from '@astrojs/tailwind';

// https://astro.build/config
export default defineConfig({
  output: 'server',
  adapter: cloudflare({
    platformProxy: {
      enabled: true,
    },
  }),
  integrations: [
    react(),
    tailwind({
      applyBaseStyles: true,
    }),
  ],
  vite: {
    ssr: {
      noExternal: ['react', 'react-dom'],
    },
    server: {
      // In local development, proxy /api/* to the Rust worker started by
      // `wrangler dev` in rust-backend/. In production both are served from
      // the same hostname via Cloudflare routes, so no proxy is needed.
      proxy: {
        '/api': {
          target: 'http://127.0.0.1:8787',
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
