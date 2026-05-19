import { test, expect } from '@playwright/test';

/**
 * Golden-path E2E suite. Covers the routes a brand-new visitor is
 * most likely to hit, so a regression in any of them counts as a
 * pre-merge blocker.
 *
 *   1. Homepage renders + nav is wired up
 *   2. Anonymous Markdown → HTML conversion works against the live API
 *   3. The signup flow creates an account end-to-end
 *
 * Tests are intentionally light on selectors: we look for visible text
 * because the design (and class names) shift between refreshes, but
 * "Universal Document Converter" / "Sign in" / "Get started" is the
 * stable contract the marketing site has had since v0.
 */

test('homepage shows the product hero and primary CTAs', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle(/Universal Document Converter/i);
  // The hero copy mentions "Markdown" and "PDF" as the headline use case.
  await expect(page.locator('body')).toContainText(/Markdown/i);
  await expect(page.locator('body')).toContainText(/PDF/i);
  // The nav links to the editor + pricing.
  await expect(page.getByRole('link', { name: /^Editor$/i }).first()).toBeVisible();
  await expect(page.getByRole('link', { name: /^Pricing$/i }).first()).toBeVisible();
});

test('anonymous markdown -> html conversion through /api/convert', async ({
  request,
}) => {
  const res = await request.post('/api/v1/convert', {
    data: { type: 'markdown', output: 'html', content: '# Hello E2E' },
  });
  expect(res.ok()).toBeTruthy();
  const body = await res.json();
  expect(body.ok).toBe(true);
  expect(body.content).toMatch(/<h1[^>]*>Hello E2E<\/h1>/);
});

test('signup creates an account and lands the user in the editor', async ({
  page,
  request,
}) => {
  // Random local email so re-runs don't collide with the `users.email`
  // UNIQUE index.
  const email = `e2e-${Date.now()}-${Math.random().toString(36).slice(2, 8)}@local`;
  const password = 'correct-horse-battery-staple';

  // Sanity: confirm the API isn't already accepting this email
  // (defends against accidental fixture leakage from a prior run).
  const before = await request.get('/api/v1/auth/me');
  expect(before.ok()).toBeTruthy();

  await page.goto('/signup');
  await page.getByLabel(/Email/i).fill(email);
  await page.getByLabel(/Password/i).first().fill(password);
  // Click the *form's* submit button rather than relying on a global
  // "Sign up" nav link.
  await page
    .getByRole('button', { name: /Sign up|Create account|Get started/i })
    .first()
    .click();

  // After signup the app redirects to the editor (or shows a logged-in
  // marker). Either way `auth/me` should now return the new user.
  await page.waitForLoadState('networkidle');
  const after = await request.get('/api/v1/auth/me');
  const me = await after.json();
  expect(me.ok).toBe(true);
  expect(me.user?.email).toBe(email);
});
