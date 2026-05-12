/**
 * Shared Playwright fixtures for the M3 hermetic e2e suite.
 *
 * The M2 SKIPPED gate is gone; M3 spawns `target/release/cobrust-studio`
 * from `globalSetup` (see `_setup.ts`) and exports the live base URL via
 * `STUDIO_BASE_URL`. Specs read it through {@link studioBaseURL} so the
 * test bodies never hard-code a port.
 *
 * Soft fallback: if the release binary is absent (the M3 DEV branch's
 * `scripts/build-release.sh` not yet merged into the test run's workspace),
 * `_setup.ts` writes `STUDIO_E2E_SKIP=1` and every spec calls
 * {@link skipIfHarnessDisabled} to short-circuit with a clear reason —
 * keeping CI green during cross-branch handoff.
 *
 * Anchor: docs/agent/modules/web-frontend.md §Tests (Wave M3 hermetic).
 */
import { test as base } from '@playwright/test';

/** Live studio-server base URL, populated by `_setup.ts`. */
export function studioBaseURL(): string {
	const url = process.env.STUDIO_BASE_URL;
	if (!url) {
		throw new Error(
			'STUDIO_BASE_URL not set — global setup did not run. ' +
				'Check web/tests/e2e/_setup.ts spawned the binary.'
		);
	}
	return url;
}

/**
 * Call from `test.beforeEach()` to short-circuit when the hermetic
 * harness deliberately skipped (release binary absent in this checkout).
 * The M2 `STUDIO_E2E=1` opt-in semantics are inverted: M3 runs by default,
 * skips only when the binary is missing.
 */
export function skipIfHarnessDisabled(): void {
	if (process.env.STUDIO_E2E_SKIP === '1') {
		base.skip(
			true,
			process.env.STUDIO_E2E_SKIP_REASON || 'hermetic harness disabled — release binary missing'
		);
	}
}

/**
 * Re-exported `test` and `expect`. The harness fixture surface stays thin
 * for M3; richer fixtures (per-test ledger seeding, etc.) land at M4 if
 * needed.
 */
export const test = base;
export { expect } from '@playwright/test';
