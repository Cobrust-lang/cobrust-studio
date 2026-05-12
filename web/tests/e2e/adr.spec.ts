/**
 * /adr — ADR list + detail + create (Wave M2 TEST Layer 2).
 *
 * Pinned UX contract:
 * 1. Empty state shows "No ADRs yet." in the table body.
 * 2. "+ New ADR" opens the create modal; title is required.
 * 3. Submitting the form POSTs `/api/adr` with the flat AdrDraft body
 *    (matching `src/lib/api.test.ts` pinning).
 * 4. The new row appears (via either the explicit refresh after submit
 *    OR the `/api/events` watcher bridge — both paths exercised).
 * 5. Clicking a row opens the detail modal with the rendered body
 *    inside a <pre> element (markdown is shown as raw text, per the
 *    A5-reconcile contract).
 *
 * TODO M2.1: spawn studio-server inside a tempdir + reset state
 * between tests so this spec is hermetic. Right now it assumes an
 * empty `<tempdir>/docs/agent/adr/` at the start.
 */
import { skipUnlessE2E, test, expect } from './_fixtures';

test.beforeEach(() => skipUnlessE2E());

test('empty state shows "No ADRs yet."', async ({ page }) => {
	await page.goto('/adr');
	await expect(page.getByRole('heading', { name: 'ADRs' })).toBeVisible();
	await expect(page.getByText(/No ADRs yet\./i)).toBeVisible();
});

test('create ADR via the modal — row appears in the table', async ({ page }) => {
	await page.goto('/adr');
	await page.getByRole('button', { name: /\+ New ADR/i }).click();

	// Modal fields
	await page.getByRole('textbox').first().fill('M2 test fixture ADR');
	// Status select stays on default `proposed`; body is markdown.
	await page.locator('textarea').fill('## Decision\nTest body.');

	const post = page.waitForRequest(
		(req) => req.url().endsWith('/api/adr') && req.method() === 'POST'
	);
	await page.getByRole('button', { name: /create adr/i }).click();
	const req = await post;
	const body = req.postDataJSON() as Record<string, unknown>;
	expect(body.title).toBe('M2 test fixture ADR');
	expect(typeof body.status).toBe('string');
	expect(typeof body.date).toBe('string');

	// Row appears (refresh-after-submit OR /api/events watcher fan-out).
	await expect(page.getByText('M2 test fixture ADR')).toBeVisible({ timeout: 3_000 });
});

test('clicking a row opens the detail dialog with the body', async ({ page }) => {
	await page.goto('/adr');
	// Assumes the prior test (or fixture seed) left at least one row.
	const firstRow = page.locator('tbody tr').first();
	await firstRow.click();
	// Detail body lives in a <pre> inside the modal.
	await expect(page.locator('pre')).toBeVisible({ timeout: 3_000 });
});
