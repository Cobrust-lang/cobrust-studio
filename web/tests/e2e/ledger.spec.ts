/**
 * /ledger — recent dispatch ledger view (Wave M2 TEST Layer 2).
 *
 * Pinned UX contract:
 * 1. Page heading "Ledger" + a count "N rows shown" derived from the
 *    fetch.
 * 2. Empty state "No entries yet." when the backing JSONL is empty.
 * 3. The `n` numeric input controls the `?n=N` query parameter on
 *    `GET /api/ledger/recent` — the unit test already pins the
 *    [0, 1000] clamp, so the e2e spec only confirms the network call
 *    carries the user-typed value.
 * 4. Refresh button re-fetches.
 *
 * Test fixture (when STUDIO_E2E=1 + a tempdir harness): pre-populate
 * `<tempdir>/.studio/ledger.jsonl` via
 * `studio_router::ledger::Ledger::append(...)` so the table shows
 * deterministic rows.
 *
 * TODO M2.1: hermetic ledger seeding — for now relies on a
 * studio-server that has had at least one dispatch made against it
 * (so the JSONL is non-empty).
 */
import { skipUnlessE2E, test, expect } from './_fixtures';

test.beforeEach(() => skipUnlessE2E());

test('ledger heading + table render', async ({ page }) => {
	await page.goto('/ledger');
	await expect(page.getByRole('heading', { name: 'Ledger' })).toBeVisible();
	await expect(page.getByText(/Timestamp \(UTC\)/i)).toBeVisible();
});

test('changing the n input drives the ?n=N query parameter', async ({ page }) => {
	await page.goto('/ledger');
	const nInput = page.locator('input[type="number"]');
	await nInput.fill('5');

	const reqPromise = page.waitForRequest((req) => req.url().includes('/api/ledger/recent?n=5'));
	await page.getByRole('button', { name: /^Refresh$/i }).click();
	const req = await reqPromise;
	expect(req.url()).toContain('n=5');
});

test('refresh button re-fetches the ledger', async ({ page }) => {
	await page.goto('/ledger');
	let count = 0;
	page.on('request', (r) => {
		if (r.url().includes('/api/ledger/recent')) count += 1;
	});
	await page.getByRole('button', { name: /^Refresh$/i }).click();
	await page.waitForTimeout(300);
	expect(count).toBeGreaterThanOrEqual(1);
});
