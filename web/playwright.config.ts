/**
 * Playwright config for the M2 frontend end-to-end suite (Wave M2 TEST).
 *
 * Harness model — three moving parts:
 *
 *   ┌─────────────────────┐    /api/*    ┌──────────────────────┐
 *   │ vite preview :4173  │ ──────────►  │ cobrust-studio :7878 │
 *   │ (adapter-static)    │   vite proxy │ --project <tempdir>  │
 *   └─────────────────────┘              └──────────────────────┘
 *           ▲
 *           │ navigation
 *           │
 *      ┌─────────┐
 *      │chromium │  Playwright drives this.
 *      └─────────┘
 *
 * Decision: we do NOT (yet) auto-spawn `cobrust-studio` from
 * `playwright.config.ts`. The Rust binary requires a Cargo build step
 * (`cargo build --bin cobrust-studio --release`) that costs minutes;
 * baking that into every `pnpm run test:e2e` would make the unit suite
 * the de-facto default. Instead, the e2e specs are gated behind the
 * `STUDIO_E2E=1` env var:
 *
 *   1. Build the binary once: `cargo build --bin cobrust-studio`
 *   2. Spawn it: `cobrust-studio serve --project $(mktemp -d) --port 7878`
 *   3. Run: `STUDIO_E2E=1 pnpm run test:e2e`
 *
 * Without `STUDIO_E2E=1`, every spec calls `test.skip(...)` at the top
 * and the suite reports "skipped" — keeping CI green until the M2.1
 * wave wires a hermetic harness (cargo-build cache + spawn-from-config).
 *
 * Anchor: docs/agent/modules/web-frontend.md §Tests.
 */
import { defineConfig, devices } from '@playwright/test';

const E2E_ENABLED = process.env.STUDIO_E2E === '1';

export default defineConfig({
	testDir: './tests/e2e',
	fullyParallel: false, // sequential — single shared studio-server backend
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 1 : 0,
	workers: 1,
	reporter: process.env.CI ? 'github' : 'list',

	use: {
		baseURL: process.env.PLAYWRIGHT_BASE_URL ?? 'http://localhost:4173',
		trace: 'retain-on-failure',
		screenshot: 'only-on-failure'
	},

	projects: [
		{
			name: 'chromium',
			use: { ...devices['Desktop Chrome'] }
		}
	],

	// The web frontend's preview server. Only started when e2e is enabled —
	// otherwise `pnpm run test:e2e` does no setup work and reports skips fast.
	webServer: E2E_ENABLED
		? {
				command: 'pnpm run build && pnpm run preview --host 127.0.0.1 --port 4173',
				url: 'http://127.0.0.1:4173',
				reuseExistingServer: !process.env.CI,
				timeout: 120_000
			}
		: undefined
});
