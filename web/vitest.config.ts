/**
 * Vitest config for M2 frontend unit tests (Wave M2 TEST).
 *
 * Scope:
 * - Pure-TS modules under `src/lib/` (api.ts, crypto.ts, util.ts, types.ts).
 * - Browser-only globals (WebCrypto, fetch, TextEncoder) are required by
 *   `crypto.ts` and the SSE parser in `api.ts`, so we run on the `jsdom`
 *   environment.
 * - Tests live next to the module they cover (`*.test.ts`).
 * - **Excluded**: `tests/e2e/**` — those are Playwright specs, not Vitest.
 *
 * M2 TEST deliberately does not pull `@testing-library/svelte` — none of
 * the unit tests render components. The five page components are
 * exercised by Layer 2 (Playwright) in `tests/e2e/`.
 *
 * Anchor: docs/agent/modules/web-frontend.md §Tests (added Wave M2 TEST).
 */
import { defineConfig } from 'vitest/config';
import path from 'node:path';

export default defineConfig({
	resolve: {
		alias: {
			$lib: path.resolve(__dirname, './src/lib')
		}
	},
	test: {
		environment: 'jsdom',
		globals: false,
		include: ['src/**/*.{test,spec}.ts'],
		exclude: ['tests/e2e/**', 'node_modules/**', 'build/**', '.svelte-kit/**'],
		setupFiles: ['./src/test-setup.ts']
	}
});
