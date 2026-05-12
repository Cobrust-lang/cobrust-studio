/**
 * Playwright globalTeardown — Wave M3 dogfood project counterpart.
 *
 * No tempdir to remove (the dogfood server runs against the repo root);
 * just kill the spawned child. Same SIGINT-then-SIGKILL escalation as
 * `_teardown.ts`. Never throws — Playwright already reported any spec
 * failure.
 */
interface HarnessState {
	child?: import('node:child_process').ChildProcess;
	port?: number;
}
const STATE_KEY = '__studioDogfoodHarness';
function harnessState(): HarnessState {
	const g = globalThis as unknown as Record<string, HarnessState | undefined>;
	return (g[STATE_KEY] as HarnessState) ?? {};
}

export default async function globalTeardown(): Promise<void> {
	const { child } = harnessState();
	if (!child || child.killed) return;
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
