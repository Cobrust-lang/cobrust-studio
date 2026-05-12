/**
 * Svelte 5 runes-based reactive store for boot-time + per-page state.
 *
 * The `appState` singleton is populated by the root `+layout.ts` load
 * function on mount and refreshed on demand. Pages read fields off it
 * directly — `$state` runes propagate updates without an explicit
 * subscribe step.
 */

import type { ProjectCurrent, VersionInfo } from './types';

class AppStore {
	project = $state<ProjectCurrent | null>(null);
	version = $state<VersionInfo | null>(null);
	/**
	 * Best-effort indicator for the "router configured" banner. The
	 * server doesn't expose a dedicated endpoint yet — we infer the
	 * state from the first dispatch attempt (503 → false; success → true)
	 * and from `/api/version` succeeding (server-alive but indeterminate
	 * → null).
	 */
	routerConfigured = $state<boolean | null>(null);

	setProject(p: ProjectCurrent | null) {
		this.project = p;
	}
	setVersion(v: VersionInfo | null) {
		this.version = v;
	}
	setRouterConfigured(state: boolean | null) {
		this.routerConfigured = state;
	}
}

export const appState = new AppStore();
