/**
 * /login — endpoint configuration flow (Wave M6 hermetic).
 *
 * Runs against the live `cobrust-studio` spawned by `_setup.ts`. No
 * manual backend setup required; the binary embeds the SvelteKit SPA
 * so the page navigations resolve same-origin.
 *
 * Pinned UX contract (ADR-0007 M6 AEAD round-trip):
 * 1. Four inputs (Base URL / API key / Model / Passphrase) and an
 *    "Unlock session" button. OAuth tab is disabled.
 * 2. Empty submit shows the toast "all fields required (including
 *    passphrase)".
 * 3. Passphrase < 8 chars shows the toast "passphrase must be ≥ 8
 *    characters".
 * 4. Valid submit POSTs `{endpoint, api_key, model, passphrase}`
 *    plaintext JSON to `/api/login`. On success the server derives
 *    the AEAD key in-process, the toast reads "session unlocked",
 *    and the page redirects to `/adr` after 400 ms.
 *
 * Plaintext-over-TLS is the contract — the server runs Argon2id
 * server-side (per ADR-0007 Option B) rather than client-side
 * WebCrypto. The old M2 `setEndpoint` client-side stub still exists
 * under `$lib/crypto.ts` but is no longer in the live UX path.
 */
import { skipIfHarnessDisabled, test, expect } from './_fixtures';

test.beforeEach(() => skipIfHarnessDisabled());

test('login form validates required fields before POSTing', async ({ page }) => {
	await page.goto('/login');
	await expect(page.getByText('Cobrust Studio')).toBeVisible();

	// Submit with all fields blank — the form short-circuits client-side.
	await page.getByPlaceholder('sk-…').fill('');
	await page.getByRole('button', { name: /unlock session/i }).click();
	await expect(page.getByText(/all fields required/i)).toBeVisible();
});

test('login form rejects passphrases shorter than 8 chars (client-side)', async ({ page }) => {
	await page.goto('/login');
	await page.getByPlaceholder('https://api.anthropic.com').fill('https://api.anthropic.com');
	await page.getByPlaceholder('sk-…').fill('sk-test-fixture-key');
	await page.getByPlaceholder('claude-opus-4-7').fill('claude-opus-4-7');
	await page.getByPlaceholder(/used to derive/i).fill('short');
	await page.getByRole('button', { name: /unlock session/i }).click();
	await expect(page.getByText(/must be ≥ 8 characters/i)).toBeVisible();
});

test('successful login posts plaintext to /api/login and redirects to /adr', async ({ page }) => {
	await page.goto('/login');
	await page.getByPlaceholder('https://api.anthropic.com').fill('https://api.anthropic.com');
	await page.getByPlaceholder('sk-…').fill('sk-test-fixture-key');
	await page.getByPlaceholder('claude-opus-4-7').fill('claude-opus-4-7');
	// Use the same passphrase as login-aead.spec.ts to avoid the
	// wrong-passphrase guard when both specs run against the same
	// hermetic binary (session_kv blob persists across tests).
	await page.getByPlaceholder(/used to derive/i).fill('playwright-test-passphrase-m6');

	// Capture the request body — pin the (endpoint, api_key, model,
	// passphrase) plaintext shape (ADR-0007 §"API surface change").
	const postPromise = page.waitForRequest(
		(req) => req.url().endsWith('/api/login') && req.method() === 'POST'
	);
	await page.getByRole('button', { name: /unlock session/i }).click();
	const req = await postPromise;
	const body = req.postDataJSON() as Record<string, unknown>;
	expect(body.endpoint).toBe('https://api.anthropic.com');
	expect(body.api_key).toBe('sk-test-fixture-key');
	expect(body.model).toBe('claude-opus-4-7');
	expect(typeof body.passphrase).toBe('string');
	expect((body.passphrase as string).length).toBeGreaterThanOrEqual(8);

	// Server returns 200 → toast "session unlocked" → goto /adr after 400ms.
	await expect(page.getByText(/session unlocked/i)).toBeVisible({ timeout: 2_000 });
	await expect(page).toHaveURL(/\/adr$/, { timeout: 3_000 });
});

test('OAuth tab is disabled (deferred to v0.5.0)', async ({ page }) => {
	await page.goto('/login');
	const oauth = page.getByRole('button', { name: /OAuth/i });
	await expect(oauth).toBeDisabled();
});
