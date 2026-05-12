/**
 * Playwright config — Wave M3 dogfood project (driven by
 * `pnpm run test:e2e:dogfood`).
 *
 * Why a separate config file (rather than reusing `playwright.config.ts`
 * with `--project=dogfood`)?
 *   Playwright resolves a single pair of `globalSetup` /
 *   `globalTeardown` hooks per *config*, not per project. The dogfood
 *   spec needs a different setup (spawn against the repo root, not a
 *   tempdir) than the hermetic suite, so we ship two configs and let
 *   `package.json` route the npm scripts to them.
 *
 * The `use.baseURL`, project naming, and globals are otherwise the
 * same. See `playwright.config.ts` for the harness diagram.
 *
 * Cross-branch dependency: same `target/release/cobrust-studio` as the
 * hermetic config; missing-binary fallback identical
 * (`STUDIO_E2E_SKIP=1` → spec short-circuits with a clear reason).
 *
 * Anchor: CLAUDE.md §6 (M3 done-means); docs/agent/modules/web-frontend.md §Tests.
 */
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
	testDir: './tests/e2e',
	testMatch: /dogfood\.spec\.ts$/,
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 1 : 0,
	workers: 1,
	reporter: process.env.CI ? 'github' : 'list',

	use: {
		baseURL: process.env.STUDIO_DOGFOOD_URL ?? 'http://127.0.0.1:7879',
		trace: 'retain-on-failure',
		screenshot: 'only-on-failure'
	},

	projects: [
		{
			name: 'dogfood',
			use: { ...devices['Desktop Chrome'] }
		}
	],

	globalSetup: './tests/e2e/_setup-dogfood.ts',
	globalTeardown: './tests/e2e/_teardown-dogfood.ts'
});
