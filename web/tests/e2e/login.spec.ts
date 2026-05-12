/**
 * /login — endpoint configuration flow (Wave M2 TEST Layer 2).
 *
 * Pre-requisites (`STUDIO_E2E=1`):
 * - `cobrust-studio serve --project <tempdir> --port 7878` running.
 * - SvelteKit preview on http://localhost:4173 (handled by Playwright
 *   `webServer` in `playwright.config.ts`).
 *
 * Pinned UX contract:
 * 1. Three inputs (Base URL / API key / Model) and a "Save endpoint"
 *    button. OAuth tab is disabled.
 * 2. Empty submit shows the toast "all fields required".
 * 3. Valid submit calls `POST /api/auth/set-endpoint` with the
 *    `EncryptedBlob` triple and redirects to `/adr` after the
 *    400ms success toast.
 * 4. Server failure surfaces the `{code}: {message}` toast verbatim.
 *
 * TODO M2.1: spawn studio-server inside the harness so the spec is
 * hermetic. Until then `STUDIO_E2E=1` requires a manually-run backend.
 */
import { skipUnlessE2E, test, expect } from './_fixtures';

test.beforeEach(() => skipUnlessE2E());

test('login form validates required fields before POSTing', async ({ page }) => {
	await page.goto('/login');
	await expect(page.getByText('Cobrust Studio')).toBeVisible();

	// Submit with blank fields — the form short-circuits client-side.
	await page.getByPlaceholder('sk-…').fill('');
	await page.getByRole('button', { name: /save endpoint/i }).click();
	await expect(page.getByText('all fields required')).toBeVisible();
});

test('successful endpoint save redirects to /adr', async ({ page }) => {
	await page.goto('/login');
	await page.getByPlaceholder('https://api.anthropic.com').fill('https://api.anthropic.com');
	await page.getByPlaceholder('sk-…').fill('sk-test-fixture-key');
	await page.getByPlaceholder('claude-opus-4-7').fill('claude-opus-4-7');

	// Capture the request body so we pin the `EncryptedBlob` triple.
	const postPromise = page.waitForRequest(
		(req) => req.url().endsWith('/api/auth/set-endpoint') && req.method() === 'POST'
	);
	await page.getByRole('button', { name: /save endpoint/i }).click();
	const req = await postPromise;
	const body = req.postDataJSON() as Record<string, unknown>;
	expect(typeof body.ciphertext).toBe('string');
	expect(typeof body.nonce).toBe('string');
	expect(body.scheme).toBe('aes-gcm-256/m2-stub');

	// Server returns 200 → toast "endpoint stored" → goto /adr after 400ms.
	await expect(page.getByText(/endpoint stored/i)).toBeVisible({ timeout: 2_000 });
	await expect(page).toHaveURL(/\/adr$/, { timeout: 3_000 });
});

test('OAuth tab is disabled (deferred to v0.5.0)', async ({ page }) => {
	await page.goto('/login');
	const oauth = page.getByRole('button', { name: /OAuth/i });
	await expect(oauth).toBeDisabled();
});
