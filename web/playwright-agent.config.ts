/**
 * Client-only Playwright coverage for /agent timeline UI.
 *
 * Serves the current SvelteKit checkout directly so M11 agent-turn UI
 * coverage tracks source changes without depending on embedded release
 * assets in `target/release/cobrust-studio`.
 */
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
	testDir: './tests/e2e',
	testMatch: /agent\.spec\.ts$/,
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 1 : 0,
	workers: 1,
	reporter: process.env.CI ? 'github' : 'list',

	use: {
		...devices['Desktop Chrome'],
		baseURL: 'http://127.0.0.1:4175',
		trace: 'retain-on-failure',
		screenshot: 'only-on-failure'
	},

	webServer: {
		command: 'pnpm dev --host 127.0.0.1 --port 4175',
		url: 'http://127.0.0.1:4175/agent',
		reuseExistingServer: !process.env.CI,
		timeout: 60_000
	}
});
