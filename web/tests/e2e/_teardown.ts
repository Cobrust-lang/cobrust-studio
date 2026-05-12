/**
 * Playwright globalTeardown — Wave M3 hermetic harness cleanup.
 *
 * Counterpart to `_setup.ts`. Runs once after every spec completes
 * (success OR failure). Reaches into the module-level harness state
 * stashed on `globalThis`, kills the spawned `cobrust-studio` child,
 * waits for the exit, then removes the tempdir.
 *
 * Failure handling: if the child is already dead (e.g. setup crashed
 * mid-spawn), `kill()` is a no-op and we proceed to the cleanup. We
 * never throw from teardown — Playwright already reported the spec
 * failure (if any) and surfacing a teardown error on top buries the
 * real signal.
 */
import { rm } from 'node:fs/promises';

interface HarnessState {
	child?: import('node:child_process').ChildProcess;
	tempdir?: string;
	port?: number;
}
const STATE_KEY = '__studioE2EHarness';

function harnessState(): HarnessState {
	const g = globalThis as unknown as Record<string, HarnessState | undefined>;
	return (g[STATE_KEY] as HarnessState) ?? {};
}

export default async function globalTeardown(): Promise<void> {
	const { child, tempdir } = harnessState();

	if (child && !child.killed) {
		// SIGINT lets the binary flush its tracing layer; fall back to
		// SIGKILL after a 2s grace period.
		const exited = new Promise<void>((res) => {
			child.once('exit', () => res());
		});
		try {
			child.kill('SIGINT');
		} catch {
			/* already gone */
		}
		const timer = new Promise<void>((res) => setTimeout(res, 2_000));
		await Promise.race([exited, timer]);
		if (!child.killed) {
			try {
				child.kill('SIGKILL');
			} catch {
				/* ignore */
			}
		}
	}

	if (tempdir) {
		try {
			await rm(tempdir, { recursive: true, force: true });
		} catch (e) {
			if (process.env.STUDIO_E2E_DEBUG === '1') {
				// eslint-disable-next-line no-console
				console.warn(`[studio-e2e] tempdir cleanup failed: ${String(e)}`);
			}
		}
	}
}
