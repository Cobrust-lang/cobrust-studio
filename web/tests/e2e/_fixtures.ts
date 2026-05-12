/**
 * Shared Playwright fixtures for the M2 e2e suite.
 *
 * `STUDIO_E2E=1` gates every spec — when unset, each spec calls
 * `test.skip(true, "...")` at the top so the suite passes with explicit
 * skips instead of failing on a missing backend. The CI / CTO can flip
 * the flag once the M2.1 wave introduces a hermetic harness.
 *
 * The shared API base URL defaults to the dev-mode vite proxy target
 * (`http://127.0.0.1:7878`); override with `STUDIO_API_BASE`.
 */
import { test as base } from '@playwright/test';

export const STUDIO_E2E_ENABLED = process.env.STUDIO_E2E === '1';
export const STUDIO_API_BASE = process.env.STUDIO_API_BASE ?? 'http://127.0.0.1:7878';

/**
 * Use at the top of every e2e file to short-circuit when the harness
 * is not wired. Keeps the suite green in default `pnpm run test:e2e`
 * runs while preserving the spec as living documentation.
 */
export function skipUnlessE2E(why = 'STUDIO_E2E=1 not set — M2.1 harness wiring TODO'): void {
	if (!STUDIO_E2E_ENABLED) base.skip(true, why);
}

/**
 * Light wrapper around the default Playwright `test` object — extension
 * surface kept minimal for M2; M2.1 will add tempdir + spawn fixtures.
 */
export const test = base;
export { expect } from '@playwright/test';
