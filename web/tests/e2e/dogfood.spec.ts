/**
 * /adr — dogfood smoke spec (Wave M3 binding "done means" test).
 *
 * Per CLAUDE.md §6: "M3 done means Studio manages its own ADRs via
 * Studio UI". This spec spawns the release binary against the Cobrust
 * Studio repo root (the constitutional corpus), navigates to `/adr`,
 * and asserts every accepted constitutional ADR title appears in the
 * rendered table. If any of the six is missing, dogfood is broken.
 *
 * Why a separate Playwright project (vs. the hermetic suite)?
 *   The hermetic specs run against an empty tempdir so they're
 *   deterministic. Dogfood is the inverse — it asserts behaviour
 *   against a fixed, repo-rooted corpus that we control via the same
 *   git history this test lives in. Running both in the same project
 *   would force one to pollute the other.
 *
 * Driver: `pnpm run test:e2e:dogfood`. The harness in
 * `_setup-dogfood.ts` exports `STUDIO_DOGFOOD_URL` (and rebinds
 * `STUDIO_BASE_URL` for convenience).
 *
 * Cross-branch dependency: requires `target/release/cobrust-studio`
 * with rust-embed enabled (M3 DEV branch's `embed.rs` + adapter-static
 * build artefact). Without that the spec falls through to the
 * `skipIfHarnessDisabled` short-circuit.
 *
 * Anchor: CLAUDE.md §6; docs/agent/modules/web-frontend.md §Tests.
 */
import { skipIfHarnessDisabled, test, expect } from './_fixtures';

test.beforeEach(() => skipIfHarnessDisabled());

/**
 * Six constitutional ADR titles, matched by substring so a minor copy
 * tweak in the frontmatter doesn't blow up the test. The substrings are
 * load-bearing on the *concept* — if any of these drops out of the
 * accepted set, dogfood is misaligned with CLAUDE.md.
 */
const CONSTITUTIONAL_ADRS: { pattern: RegExp; rationale: string }[] = [
	{ pattern: /Stack choice/i, rationale: 'ADR-0001' },
	{ pattern: /Single-binary deployment/i, rationale: 'ADR-0002' },
	{ pattern: /custom-endpoint-first/i, rationale: 'ADR-0003 (auth)' },
	{ pattern: /Storage/i, rationale: 'ADR-0004' },
	{ pattern: /Agent runner/i, rationale: 'ADR-0005' },
	{ pattern: /studio-router public API/i, rationale: 'ADR-0006' }
];

test('/adr renders the 6 constitutional ADRs from the live repo', async ({ page }) => {
	await page.goto('/adr');
	await expect(page.getByRole('heading', { name: 'ADRs' })).toBeVisible();

	// Wait for the table to populate (the page issues `GET /api/adr` on
	// mount; allow a generous timeout for the rust-embed + filesystem
	// walk to settle on the first run).
	await expect(page.locator('tbody tr').first()).toBeVisible({ timeout: 10_000 });

	for (const { pattern, rationale } of CONSTITUTIONAL_ADRS) {
		// The matcher prints the rationale on failure, so a missing ADR
		// surfaces as "expected /pattern/ — rationale ADR-000X" in the
		// trace, not a bare element-not-found.
		await expect(
			page.getByText(pattern).first(),
			`${rationale} must be visible in /adr`
		).toBeVisible({ timeout: 5_000 });
	}
});

test('/adr exposes at least 6 rows (constitutional minimum)', async ({ page }) => {
	await page.goto('/adr');
	await expect(page.locator('tbody tr').first()).toBeVisible({ timeout: 10_000 });
	const count = await page.locator('tbody tr').count();
	expect(count).toBeGreaterThanOrEqual(6);
});
