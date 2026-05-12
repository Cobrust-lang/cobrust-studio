/**
 * Root layout load function — boot-time fetch of project metadata +
 * server version. Errors are swallowed (the navbar simply renders a
 * "server unreachable" indicator) so a Studio server in mid-start
 * doesn't block the SPA from rendering.
 *
 * Adapter-static (per ADR-0002 / svelte.config.js) means this runs in
 * the browser only — `prerender = false` keeps the fetch out of the
 * build-time render path.
 */

import type { LayoutLoad } from './$types';
import { getProject, getVersion, ApiError } from '$lib/api';
import { appState } from '$lib/store.svelte';

export const prerender = false;
export const ssr = false;

export const load: LayoutLoad = async () => {
	// Fire both requests in parallel — they're independent and idempotent.
	const [project, version] = await Promise.all([
		getProject().catch((e: unknown) => {
			if (e instanceof ApiError) console.warn('getProject failed:', e.code, e.message);
			return null;
		}),
		getVersion().catch((e: unknown) => {
			if (e instanceof ApiError) console.warn('getVersion failed:', e.code, e.message);
			return null;
		})
	]);
	appState.setProject(project);
	appState.setVersion(version);
	return {};
};
