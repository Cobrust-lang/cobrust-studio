/**
 * /agent — dispatch composer + SSE live stream (Wave M3 hermetic).
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
 * The M3 hermetic harness (`_setup.ts`) writes `studio.toml` with a
 * synthetic provider when `STUDIO_E2E_ROUTER=1`, so the universe
 * switch is fully automatic now:
 *
 *   default                  → universe A (router=None)
 *   STUDIO_E2E_ROUTER=1      → universe B (synthetic provider)
 */
import { skipIfHarnessDisabled, test, expect } from './_fixtures';

const ROUTER_ENABLED = process.env.STUDIO_E2E_ROUTER === '1';

test.beforeEach(() => skipIfHarnessDisabled());

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
