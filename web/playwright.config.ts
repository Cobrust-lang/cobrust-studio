/**
 * Playwright config — Wave M3 hermetic harness + dogfood project.
 *
 *   ┌────────────────────────────────────────────────────────────────┐
 *   │  Hermetic project (`pnpm run test:e2e`)                        │
 *   │                                                                │
 *   │   _setup.ts  ─► mkdtemp + spawn target/release/cobrust-studio  │
 *   │                 (--project <tempdir> --port <random>)          │
 *   │                 │                                              │
 *   │                 ▼                                              │
 *   │   ┌─────────────────────────┐                                  │
 *   │   │ cobrust-studio :<rand>  │  ◄── chromium navigations        │
 *   │   │ rust-embed serves SPA   │     (login/adr/agent/finding/    │
 *   │   │ same-origin             │      ledger specs — 13 tests)    │
 *   │   └─────────────────────────┘                                  │
 *   │                 │                                              │
 *   │                 ▼                                              │
 *   │   _teardown.ts ◄── kill + rmdir tempdir                        │
 *   └────────────────────────────────────────────────────────────────┘
 *
 *   ┌────────────────────────────────────────────────────────────────┐
 *   │  Dogfood project (`pnpm run test:e2e:dogfood`)                 │
 *   │                                                                │
 *   │   _setup-dogfood.ts  ─► spawn against the repo root (no temp)  │
 *   │   _teardown-dogfood.ts ◄── kill child                          │
 *   │                                                                │
 *   │   dogfood.spec.ts: navigate to /adr, assert the 6              │
 *   │   constitutional ADRs are rendered. This is the M3 done-means  │
 *   │   binding test per CLAUDE.md §6.                               │
 *   └────────────────────────────────────────────────────────────────┘
 *
 * Cross-branch dependency (Wave M3):
 *   `target/release/cobrust-studio` is produced by M3 DEV's
 *   `scripts/build-release.sh`. If the binary is absent at setup
 *   time, the global-setup hook flips `STUDIO_E2E_SKIP=1` and every
 *   spec short-circuits with a clear reason — the suite reports
 *   skips (green) instead of failing on a missing binary, giving
 *   the CTO a safe re-run window after the DEV merge.
 *
 * Anchor: docs/agent/modules/web-frontend.md §Tests (Wave M3).
 */
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
	testDir: './tests/e2e',
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 1 : 0,
	workers: 1,
	reporter: process.env.CI ? 'github' : 'list',

	use: {
		// The hermetic + dogfood projects each set their own baseURL
		// after global-setup writes STUDIO_BASE_URL / STUDIO_DOGFOOD_URL.
		// The top-level fallback keeps Playwright happy if a single test
		// is dispatched without a project (e.g. `playwright test foo.spec`).
		baseURL: process.env.STUDIO_BASE_URL ?? 'http://127.0.0.1:7878',
		trace: 'retain-on-failure',
		screenshot: 'only-on-failure'
	},

	projects: [
		{
			name: 'hermetic',
			testIgnore: /dogfood\.spec\.ts$/,
			use: {
				...devices['Desktop Chrome'],
				baseURL: process.env.STUDIO_BASE_URL ?? 'http://127.0.0.1:7878'
			}
		},
		{
			name: 'dogfood',
			testMatch: /dogfood\.spec\.ts$/,
			use: {
				...devices['Desktop Chrome'],
				baseURL: process.env.STUDIO_DOGFOOD_URL ?? 'http://127.0.0.1:7879'
			}
		}
	],

	// Default global setup/teardown = hermetic. `test:e2e:dogfood` invokes
	// Playwright with `--project=dogfood` AND the dogfood setup pair via
	// the dedicated `playwright-dogfood.config.ts` (one file, one binary
	// — keeps the npm script readable).
	globalSetup: './tests/e2e/_setup.ts',
	globalTeardown: './tests/e2e/_teardown.ts'
});
