/**
 * Client-only Playwright smoke for M10 i18n.
 *
 * The default hermetic config exercises the embedded release binary. This
 * config serves the current SvelteKit checkout directly so frontend-only
 * locale work can be verified without a release rebuild.
 */
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
	testDir: './tests/e2e',
	testMatch: /i18n\.spec\.ts$/,
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 1 : 0,
	workers: 1,
	reporter: process.env.CI ? 'github' : 'list',

	use: {
		...devices['Desktop Chrome'],
		baseURL: 'http://127.0.0.1:4174',
		trace: 'retain-on-failure',
		screenshot: 'only-on-failure'
	},

	webServer: {
		command: 'pnpm dev --host 127.0.0.1 --port 4174',
		url: 'http://127.0.0.1:4174/login',
		reuseExistingServer: !process.env.CI,
		timeout: 60_000
	}
});
