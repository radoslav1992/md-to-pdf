import { defineConfig, devices } from '@playwright/test';

/**
 * E2E config. The dev server boots both the Astro frontend (port 4321)
 * and the Rust API (port 8000) via `npm run dev`; the tests hit the
 * Astro server, which proxies `/api/*` to the API in development.
 *
 * The DB is a fresh tempfile per CI run — set `DATABASE_URL` to a path
 * under `/tmp` (or wipe `rust-backend/dev.db*`) before running locally
 * if you want to start from a clean slate.
 */
export default defineConfig({
  testDir: './e2e',
  // One worker by default — these tests share account fixtures, so
  // parallel test runs across the same DB would step on each other.
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://127.0.0.1:4321',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: process.env.E2E_SKIP_WEB_SERVER
    ? undefined
    : {
        command: 'npm run dev',
        url: 'http://127.0.0.1:4321',
        // First boot has to compile the Rust backend; allow plenty of
        // time before failing the suite.
        timeout: 180_000,
        reuseExistingServer: !process.env.CI,
      },
});
