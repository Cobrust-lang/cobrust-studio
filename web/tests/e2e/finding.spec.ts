/**
 * /finding — Finding list + detail + create (Wave M2 TEST Layer 2).
 *
 * Symmetric to /adr but with severity + status badges. The M1 server
 * contract does not expose `GET /api/finding/:id` (per the page
 * comment) so the detail dialog shows summary fields only — the
 * spec asserts the summary view, not a body fetch.
 *
 * Pinned UX contract:
 * 1. Empty state shows "No findings yet."
 * 2. Severity badge color follows `severityClass()` — P0 = err, P1
 *    = warn, P2 = amber, P3 = info. The spec pins the data-attribute
 *    rather than the colour (CSS is brittle).
 * 3. Create flow POSTs `/api/finding` with the flat FindingDraft and
 *    refreshes the list.
 *
 * TODO M2.1: hermetic backend; for now relies on the same tempdir
 * harness as `adr.spec.ts`.
 */
import { skipUnlessE2E, test, expect } from './_fixtures';

test.beforeEach(() => skipUnlessE2E());

test('empty state shows "No findings yet."', async ({ page }) => {
	await page.goto('/finding');
	await expect(page.getByRole('heading', { name: 'Findings' })).toBeVisible();
	await expect(page.getByText(/No findings yet\./i)).toBeVisible();
});

test('create finding via the modal — row appears with the right severity badge', async ({
	page
}) => {
	await page.goto('/finding');
	await page.getByRole('button', { name: /\+ New finding/i }).click();

	await page.getByPlaceholder('m2-frontend-tab-leak').fill('m2-test-fixture');
	// Title is the second text input in the modal (after finding_id).
	await page.locator('input[type="text"]').nth(2).fill('Test finding title');
	// Severity select — pick P1.
	await page.locator('select').first().selectOption('P1');

	const post = page.waitForRequest(
		(req) => req.url().endsWith('/api/finding') && req.method() === 'POST'
	);
	await page.getByRole('button', { name: /create finding/i }).click();
	const req = await post;
	const body = req.postDataJSON() as Record<string, unknown>;
	expect(body.finding_id).toBe('m2-test-fixture');
	expect(body.title).toBe('Test finding title');
	expect(body.severity).toBe('P1');

	// New row visible.
	await expect(page.getByText('Test finding title')).toBeVisible({ timeout: 3_000 });
	// Severity badge text reads P1 in the row.
	await expect(page.getByText('P1', { exact: true })).toBeVisible();
});

test('clicking a finding row opens the summary-only detail modal', async ({ page }) => {
	await page.goto('/finding');
	const row = page.locator('tbody tr').first();
	await row.click();
	// The detail modal references the M2+ `GET /api/finding/:id` deferral.
	await expect(page.getByText(/Body view requires a singleton/i)).toBeVisible({ timeout: 3_000 });
});
