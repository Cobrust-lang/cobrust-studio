/**
 * Playwright globalSetup — Wave M3 hermetic harness for the e2e suite.
 *
 * Lifecycle (per `pnpm run test:e2e` session, project = `hermetic`):
 *
 *   1. mkdtemp `/tmp/studio-e2e-XXXX/` for the project root.
 *   2. If `STUDIO_E2E_ROUTER=1`, drop a `studio.toml` declaring a
 *      synthetic provider so `/api/dispatch` returns 200 SSE (not 503).
 *      The synthetic-only shape is the same one exercised by
 *      `crates/studio-server/src/router_init.rs::returns_some_with_synthetic_only_config`.
 *   3. Spawn `target/release/cobrust-studio serve --project <tempdir>
 *      --port <random> --host 127.0.0.1`.
 *   4. Poll `GET /api/health` until 200 (up to 30s).
 *   5. Export `STUDIO_BASE_URL=http://127.0.0.1:<port>` so the
 *      `hermetic` project's `baseURL` resolves to the live server. The
 *      embedded-asset path (rust-embed in `target/release`) serves the
 *      built SvelteKit SPA from the same origin — no `vite preview`
 *      needed.
 *
 * The teardown step (`_teardown.ts`) kills the child + removes tempdir.
 *
 * Soft-fallback contract (cross-branch dependency):
 *   `target/release/cobrust-studio` is produced by M3 DEV's
 *   `scripts/build-release.sh`. While that script is still on the
 *   `feature/m3-dev-embed-dogfood` branch, this checkout may not have
 *   the binary. We DETECT the absence at setup time and set
 *   `STUDIO_E2E_SKIP=1` so every spec's `skipIfHarnessDisabled` fires
 *   with a clear reason — turning a missing-binary failure into a
 *   graceful skip the CTO can re-run after the dev merge.
 *
 * Anchor: docs/agent/modules/web-frontend.md §Tests (Wave M3 hermetic).
 */
import { spawn, type ChildProcess } from 'node:child_process';
import { createServer } from 'node:net';
import { mkdtemp, writeFile, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** Module-level state so teardown can find the child + tempdir. */
interface HarnessState {
	child?: ChildProcess;
	tempdir?: string;
	port?: number;
}
const STATE_KEY = '__studioE2EHarness';
function harnessState(): HarnessState {
	const g = globalThis as unknown as Record<string, HarnessState | undefined>;
	if (!g[STATE_KEY]) g[STATE_KEY] = {};
	return g[STATE_KEY] as HarnessState;
}

/**
 * Repo root (one level above `web/`). ESM-safe: resolves from
 * `import.meta.url` so the path is stable whether Playwright loads
 * this file as CJS (its default TS transformer) or ESM.
 */
function repoRoot(): string {
	const here = dirname(fileURLToPath(import.meta.url));
	return resolve(here, '../../..');
}

/** Path the release binary is expected at. */
function binaryPath(): string {
	return join(repoRoot(), 'target', 'release', 'cobrust-studio');
}

/** Pick a free localhost port by binding `0`, reading, closing. */
async function pickFreePort(): Promise<number> {
	return new Promise<number>((res, rej) => {
		const srv = createServer();
		srv.on('error', rej);
		srv.listen(0, '127.0.0.1', () => {
			const addr = srv.address();
			if (addr && typeof addr === 'object') {
				const port = addr.port;
				srv.close(() => res(port));
			} else {
				srv.close();
				rej(new Error('failed to acquire ephemeral port'));
			}
		});
	});
}

/** Poll `GET /api/health` until 200; throw after `timeoutMs`. */
async function waitForHealth(baseUrl: string, timeoutMs = 30_000): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	let lastErr: unknown = null;
	while (Date.now() < deadline) {
		try {
			const r = await fetch(`${baseUrl}/api/health`);
			if (r.ok) return;
			lastErr = new Error(`health returned ${r.status}`);
		} catch (e) {
			lastErr = e;
		}
		await new Promise((r) => setTimeout(r, 200));
	}
	throw new Error(
		`studio-server did not become healthy within ${timeoutMs}ms — last error: ${String(lastErr)}`
	);
}

/** Synthetic-only studio.toml — synthesises an SSE-200 path for /api/dispatch. */
const SYNTHETIC_STUDIO_TOML = `
[router]
strategy = "quality"
preferred = ["synth_a:synthetic-1"]

[providers.synth_a]
kind = "synthetic"
base_url = "https://example.invalid"
api_key_env = ""
models = ["synthetic-1"]
`;

/**
 * Entry point. Playwright invokes this once before any test starts
 * (when configured via `playwright.config.ts::globalSetup`).
 *
 * On success: sets `STUDIO_BASE_URL` in `process.env` for the worker
 * processes Playwright spawns. On binary-absent: sets
 * `STUDIO_E2E_SKIP=1` and `STUDIO_E2E_SKIP_REASON=...` so each spec's
 * `beforeEach` short-circuits.
 */
export default async function globalSetup(): Promise<void> {
	const state = harnessState();

	// 1. Verify binary exists. Soft-fail if not.
	const bin = binaryPath();
	try {
		await access(bin);
	} catch {
		process.env.STUDIO_E2E_SKIP = '1';
		process.env.STUDIO_E2E_SKIP_REASON =
			`release binary missing at ${bin} — run \`bash scripts/build-release.sh\` ` +
			`(M3 DEV branch) or land the script in this checkout first`;
		// eslint-disable-next-line no-console
		console.warn(`[studio-e2e] ${process.env.STUDIO_E2E_SKIP_REASON} — skipping all specs`);
		return;
	}

	// 2. tempdir + optional studio.toml seed.
	const tempdir = await mkdtemp(join(tmpdir(), 'studio-e2e-'));
	state.tempdir = tempdir;
	if (process.env.STUDIO_E2E_ROUTER === '1') {
		await writeFile(join(tempdir, 'studio.toml'), SYNTHETIC_STUDIO_TOML.trimStart());
	}

	// 3. Pick a port, spawn the server.
	const port = await pickFreePort();
	state.port = port;
	const child = spawn(
		bin,
		['serve', '--project', tempdir, '--host', '127.0.0.1', '--port', String(port)],
		{
			stdio: ['ignore', 'pipe', 'pipe'],
			env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? 'warn' }
		}
	);
	state.child = child;

	// Surface unexpected stderr (helps debug a quick exit).
	child.stderr?.on('data', (buf: Buffer) => {
		if (process.env.STUDIO_E2E_DEBUG === '1') {
			process.stderr.write(`[studio-server] ${buf.toString()}`);
		}
	});

	const earlyExit = new Promise<never>((_, rej) => {
		child.once('exit', (code, sig) => {
			rej(new Error(`studio-server exited early (code=${code} signal=${sig})`));
		});
	});

	const baseUrl = `http://127.0.0.1:${port}`;

	// 4. Wait for /api/health.
	await Promise.race([waitForHealth(baseUrl), earlyExit]);

	// 5. Export env for the test workers.
	process.env.STUDIO_BASE_URL = baseUrl;
	if (process.env.STUDIO_E2E_DEBUG === '1') {
		// eslint-disable-next-line no-console
		console.log(`[studio-e2e] harness ready — base=${baseUrl} tempdir=${tempdir}`);
	}
}
