/**
 * /agent — dispatch composer + SSE live stream (Wave M2 TEST Layer 2).
 *
 * Two universes:
 *
 *   A. router = None — server returns 503 `router_not_configured` for
 *      every `POST /api/dispatch`. The page shows a warning "LLM router
 *      not configured" with a link to /login.
 *
 *   B. router = Some(SyntheticProvider) — the SSE stream emits ≥1
 *      `chunk` frames followed by a terminal `done` frame. The page
 *      appends each chunk to the transcript and renders a usage badge
 *      from the done payload.
 *
 * Both universes are exercised here, gated by env:
 *
 *   STUDIO_E2E=1            → universe A by default
 *   STUDIO_E2E_ROUTER=1     → universe B (test fixture provides
 *                             studio.toml with a synthetic provider)
 *
 * TODO M2.1: write the `studio.toml` from the harness so the spec
 * doesn't need an out-of-band setup step.
 */
import { skipUnlessE2E, test, expect } from './_fixtures';

const ROUTER_ENABLED = process.env.STUDIO_E2E_ROUTER === '1';

test.beforeEach(() => skipUnlessE2E());

test('router = None → page shows the "Configure LLM endpoint" CTA', async ({ page }) => {
	test.skip(ROUTER_ENABLED, 'router-on universe — see the next test');
	await page.goto('/agent');
	await page.locator('textarea').first().fill('test prompt');
	await page.getByRole('button', { name: /^Dispatch$/i }).click();

	// 503 router_not_configured → routerMissing = true → CTA visible.
	await expect(page.getByText(/LLM router not configured/i)).toBeVisible({ timeout: 5_000 });
	await expect(page.getByRole('link', { name: /Configure endpoint/i })).toHaveAttribute(
		'href',
		'/login'
	);
});

test('router = Some(synthetic) → SSE stream renders chunks + usage badge', async ({ page }) => {
	test.skip(!ROUTER_ENABLED, 'router-off universe — STUDIO_E2E_ROUTER=1 to enable');
	await page.goto('/agent');
	await page.locator('textarea').nth(1).fill('Say hello in three words.');
	await page.getByRole('button', { name: /^Dispatch$/i }).click();

	// Wait for the streaming output to populate.
	const transcript = page.locator('pre');
	await expect(transcript).not.toHaveText(/Dispatch to populate/, { timeout: 10_000 });

	// Done frame surfaces the provider + token usage badge.
	await expect(page.getByText(/tokens:/i)).toBeVisible({ timeout: 10_000 });
});
