/**
 * Playwright globalSetup — Wave M3 dogfood project.
 *
 * Same lifecycle as `_setup.ts` BUT the `--project` flag points at the
 * Cobrust Studio repo root itself (no tempdir). This is the binding
 * "M3 done means" test per CLAUDE.md §6 — Studio manages its own
 * ADR/finding/ledger corpus through its own UI. The spec at
 * `dogfood.spec.ts` then asserts the 6 constitutional ADRs are visible
 * in the rendered `/adr` table.
 *
 * Soft-fallback: identical to `_setup.ts`. Missing binary → set
 * `STUDIO_E2E_SKIP=1` + reason so the dogfood spec also short-circuits
 * cleanly during cross-branch handoff.
 */
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { access } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

interface HarnessState {
	child?: import('node:child_process').ChildProcess;
	port?: number;
}
const STATE_KEY = '__studioDogfoodHarness';
function harnessState(): HarnessState {
	const g = globalThis as unknown as Record<string, HarnessState | undefined>;
	if (!g[STATE_KEY]) g[STATE_KEY] = {};
	return g[STATE_KEY] as HarnessState;
}

/**
 * Repo root (one level above `web/`). ESM-safe — see `_setup.ts`.
 */
function repoRoot(): string {
	const here = dirname(fileURLToPath(import.meta.url));
	return resolve(here, '../../..');
}
function binaryPath(): string {
	return join(repoRoot(), 'target', 'release', 'cobrust-studio');
}

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

export default async function globalSetup(): Promise<void> {
	const state = harnessState();
	const bin = binaryPath();
	try {
		await access(bin);
	} catch {
		process.env.STUDIO_E2E_SKIP = '1';
		process.env.STUDIO_E2E_SKIP_REASON =
			`release binary missing at ${bin} — run \`bash scripts/build-release.sh\` ` +
			`(M3 DEV branch) or land the script in this checkout first`;
		// eslint-disable-next-line no-console
		console.warn(`[studio-dogfood] ${process.env.STUDIO_E2E_SKIP_REASON} — skipping dogfood spec`);
		return;
	}

	const port = await pickFreePort();
	state.port = port;
	const child = spawn(
		bin,
		['serve', '--project', repoRoot(), '--host', '127.0.0.1', '--port', String(port)],
		{
			stdio: ['ignore', 'pipe', 'pipe'],
			env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? 'warn' }
		}
	);
	state.child = child;
	child.stderr?.on('data', (buf: Buffer) => {
		if (process.env.STUDIO_E2E_DEBUG === '1') {
			process.stderr.write(`[studio-dogfood] ${buf.toString()}`);
		}
	});
	const earlyExit = new Promise<never>((_, rej) => {
		child.once('exit', (code, sig) => {
			rej(new Error(`studio-server (dogfood) exited early (code=${code} signal=${sig})`));
		});
	});

	const baseUrl = `http://127.0.0.1:${port}`;
	await Promise.race([waitForHealth(baseUrl), earlyExit]);
	process.env.STUDIO_DOGFOOD_URL = baseUrl;
	// Also export STUDIO_BASE_URL because the dogfood spec uses the same
	// fixtures-style getter; if both projects run sequentially, this is
	// overwritten by the second setup safely.
	process.env.STUDIO_BASE_URL = baseUrl;
	if (process.env.STUDIO_E2E_DEBUG === '1') {
		// eslint-disable-next-line no-console
		console.log(`[studio-dogfood] harness ready — base=${baseUrl} project=${repoRoot()}`);
	}
}
