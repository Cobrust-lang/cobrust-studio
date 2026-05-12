/**
 * Unit tests for the typed fetch + SSE wrapper in `src/lib/api.ts`.
 *
 * These tests mock `globalThis.fetch` directly — Vitest's `vi.fn()` is
 * sufficient for the surface area we cover. We deliberately avoid pulling
 * `msw` / `nock` for M2; the API is small and the mock is one-line.
 *
 * Wire contracts pinned (each test maps to a section in
 * `docs/agent/modules/studio-server.md`):
 * - `GET /api/adr` → `{ adrs: AdrSummary[] }` envelope
 * - `POST /api/adr` carries the flat `AdrDraft` body
 * - `GET /api/finding` → `{ findings: FindingSummary[] }`
 * - `POST /api/finding` carries the flat `FindingDraft`
 * - `GET /api/ledger/recent?n=N` clamp behaviour (0..1000)
 * - `POST /api/auth/set-endpoint` carries `EncryptedBlob`
 * - `POST /api/dispatch` SSE frame parsing (chunk → done | error)
 * - Error envelope `{error, code}` round-trips into `ApiError`
 * - 503 `router_not_configured` surfaces with the matching code
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	ApiError,
	createAdr,
	createFinding,
	dispatchSse,
	getAdr,
	getProject,
	getVersion,
	listAdrs,
	listFindings,
	recentLedger,
	setEndpoint,
	type DispatchEvent
} from './api';
import type {
	Adr,
	AdrDraft,
	DispatchRequest,
	EncryptedBlob,
	FindingDraft,
	LedgerEntry
} from './types';

// ───── Test helpers ─────────────────────────────────────────────────

type FetchInput = Parameters<typeof fetch>[0];
type FetchInit = Parameters<typeof fetch>[1];

interface RecordedCall {
	url: string;
	init: FetchInit | undefined;
}

let calls: RecordedCall[] = [];

/** Install a fetch mock that returns the given response for every call. */
function mockJsonResponse(body: unknown, init: { status?: number } = {}): void {
	const resp = new Response(JSON.stringify(body), {
		status: init.status ?? 200,
		headers: { 'content-type': 'application/json' }
	});
	globalThis.fetch = vi.fn((input: FetchInput, fetchInit?: FetchInit) => {
		calls.push({ url: String(input), init: fetchInit });
		return Promise.resolve(resp.clone());
	}) as unknown as typeof fetch;
}

/** Install a fetch mock that returns a streaming body for SSE tests. */
function mockSseStream(frames: string[], init: { status?: number } = {}): void {
	const body = new ReadableStream<Uint8Array>({
		start(controller) {
			const enc = new TextEncoder();
			for (const f of frames) controller.enqueue(enc.encode(f));
			controller.close();
		}
	});
	const resp = new Response(body, {
		status: init.status ?? 200,
		headers: { 'content-type': 'text/event-stream' }
	});
	globalThis.fetch = vi.fn((input: FetchInput, fetchInit?: FetchInit) => {
		calls.push({ url: String(input), init: fetchInit });
		return Promise.resolve(resp);
	}) as unknown as typeof fetch;
}

beforeEach(() => {
	calls = [];
});
afterEach(() => {
	vi.restoreAllMocks();
});

// ───── ADR ──────────────────────────────────────────────────────────

describe('listAdrs', () => {
	it('GETs /api/adr with accept: application/json and unwraps the envelope', async () => {
		mockJsonResponse({
			adrs: [
				{
					adr_id: 1,
					title: 'Adopt Rust',
					status: 'accepted',
					date: '2026-05-01',
					path: '/p/0001.md'
				}
			]
		});
		const result = await listAdrs();
		expect(calls).toHaveLength(1);
		expect(calls[0].url).toBe('/api/adr');
		const headers = (calls[0].init?.headers ?? {}) as Record<string, string>;
		expect(headers.accept).toBe('application/json');
		expect(result).toHaveLength(1);
		expect(result[0].adr_id).toBe(1);
		expect(result[0].title).toBe('Adopt Rust');
	});

	it('throws ApiError with the {error, code} envelope on non-2xx', async () => {
		mockJsonResponse(
			{ error: 'store unavailable', code: 'store_io' },
			{ status: 500 }
		);
		await expect(listAdrs()).rejects.toMatchObject({
			name: 'ApiError',
			status: 500,
			code: 'store_io',
			message: 'store unavailable'
		});
	});

	it('uses a synthetic transport_error code when fetch itself rejects', async () => {
		globalThis.fetch = vi.fn(() =>
			Promise.reject(new TypeError('network down'))
		) as unknown as typeof fetch;
		await expect(listAdrs()).rejects.toMatchObject({
			name: 'ApiError',
			status: 0,
			code: 'transport_error'
		});
	});

	it('falls back to unknown_error when the error body is not JSON', async () => {
		const resp = new Response('<html>oh no</html>', {
			status: 502,
			headers: { 'content-type': 'text/html' }
		});
		globalThis.fetch = vi.fn(() => Promise.resolve(resp)) as unknown as typeof fetch;
		const err = await listAdrs().catch((e: unknown) => e);
		expect(err).toBeInstanceOf(ApiError);
		const apiErr = err as ApiError;
		expect(apiErr.status).toBe(502);
		expect(apiErr.code).toBe('unknown_error');
	});
});

describe('getAdr', () => {
	it('GETs /api/adr/:id and returns the flat Adr shape', async () => {
		const fixture: Adr = {
			adr_id: 7,
			title: 'Use SSE',
			status: 'accepted',
			date: '2026-05-10',
			path: '/p/0007.md',
			body: '# Decision\n…',
			supersedes: [],
			superseded_by: []
		};
		mockJsonResponse(fixture);
		const got = await getAdr(7);
		expect(calls[0].url).toBe('/api/adr/7');
		expect(got).toEqual(fixture);
	});
});

describe('createAdr', () => {
	it('POSTs JSON to /api/adr with the AdrDraft body verbatim', async () => {
		const draft: AdrDraft = {
			title: 'Auth M2 stub',
			status: 'proposed',
			date: '2026-05-11',
			body: '## Context\n',
			supersedes: [3, 5]
		};
		mockJsonResponse({
			adr_id: 99,
			title: draft.title,
			status: 'proposed',
			date: draft.date,
			path: '/p/0099.md',
			body: draft.body,
			supersedes: [3, 5],
			superseded_by: []
		});
		await createAdr(draft);
		expect(calls[0].url).toBe('/api/adr');
		expect(calls[0].init?.method).toBe('POST');
		const headers = (calls[0].init?.headers ?? {}) as Record<string, string>;
		expect(headers['content-type']).toBe('application/json');
		expect(headers.accept).toBe('application/json');
		const body = JSON.parse(String(calls[0].init?.body));
		expect(body).toEqual(draft);
	});
});

// ───── Finding ──────────────────────────────────────────────────────

describe('listFindings', () => {
	it('GETs /api/finding and unwraps the { findings } envelope', async () => {
		mockJsonResponse({
			findings: [
				{
					finding_id: 'f-1',
					title: 'tab leak',
					status: 'open',
					severity: 'P2',
					date: 'abc123',
					path: '/p/findings/f-1.md'
				}
			]
		});
		const rows = await listFindings();
		expect(calls[0].url).toBe('/api/finding');
		expect(rows[0].finding_id).toBe('f-1');
	});
});

describe('createFinding', () => {
	it('POSTs the flat FindingDraft to /api/finding', async () => {
		const draft: FindingDraft = {
			finding_id: 'm2-flake-1',
			title: 'flake',
			severity: 'P3',
			status: 'open',
			last_verified_commit: 'HEAD',
			dependencies: ['adr:0006'],
			related: [],
			body: '## repro\n'
		};
		mockJsonResponse({
			...draft,
			date: 'HEAD',
			path: '/p/findings/m2-flake-1.md'
		});
		await createFinding(draft);
		expect(calls[0].url).toBe('/api/finding');
		expect(calls[0].init?.method).toBe('POST');
		const sent = JSON.parse(String(calls[0].init?.body));
		expect(sent).toEqual(draft);
	});
});

// ───── Ledger ───────────────────────────────────────────────────────

describe('recentLedger', () => {
	it('clamps the n parameter to [0, 1000] and builds the query string', async () => {
		const entry: LedgerEntry = {
			ts: '2026-05-11T00:00:00Z',
			task_tag: 'unit',
			provider: 'synthetic',
			provider_kind: 'synthetic',
			model: 'echo',
			cache_key: 'abc',
			cache_hit: false,
			prompt_tokens: 1,
			completion_tokens: 2,
			total_tokens: 3,
			latency_ms: 4,
			attempt: 1,
			outcome: 'ok',
			error_code: null
		};
		mockJsonResponse({ entries: [entry] });
		await recentLedger(20);
		expect(calls[0].url).toBe('/api/ledger/recent?n=20');

		// Defaults to 20 when no argument is provided.
		await recentLedger();
		expect(calls[1].url).toBe('/api/ledger/recent?n=20');

		// Clamps to upper bound.
		await recentLedger(999_999);
		expect(calls[2].url).toBe('/api/ledger/recent?n=1000');

		// Clamps to lower bound.
		await recentLedger(-7);
		expect(calls[3].url).toBe('/api/ledger/recent?n=0');
	});
});

// ───── Auth ─────────────────────────────────────────────────────────

describe('setEndpoint', () => {
	it('POSTs the EncryptedBlob triple to /api/auth/set-endpoint', async () => {
		const blob: EncryptedBlob = {
			ciphertext: 'AQID',
			nonce: 'AAEC',
			scheme: 'aes-gcm-256/m2-stub'
		};
		mockJsonResponse({ status: 'stored' });
		const out = await setEndpoint(blob);
		expect(calls[0].url).toBe('/api/auth/set-endpoint');
		expect(calls[0].init?.method).toBe('POST');
		expect(JSON.parse(String(calls[0].init?.body))).toEqual(blob);
		expect(out.status).toBe('stored');
	});
});

// ───── Project / Version ────────────────────────────────────────────

describe('getProject + getVersion', () => {
	it('GETs /api/project/current and /api/version', async () => {
		mockJsonResponse({
			project_root: '/tmp/p',
			started_at: '2026-05-11T00:00:00Z',
			version: '0.1.0'
		});
		const p = await getProject();
		expect(calls[0].url).toBe('/api/project/current');
		expect(p.version).toBe('0.1.0');

		mockJsonResponse({
			studio_server: '0.1.0',
			studio_store: '0.1.0',
			studio_router: '0.1.0',
			rustc: '1.85'
		});
		const v = await getVersion();
		// Second fetch (mockJsonResponse keeps `calls` across both mocks).
		expect(calls[1].url).toBe('/api/version');
		expect(v.studio_server).toBe('0.1.0');
	});
});

// ───── Dispatch SSE ─────────────────────────────────────────────────

/**
 * Collect every yielded event from `dispatchSse` into a flat array.
 * The generator is async so we use `for await`.
 */
async function collect(req: DispatchRequest): Promise<DispatchEvent[]> {
	const out: DispatchEvent[] = [];
	for await (const evt of dispatchSse(req)) out.push(evt);
	return out;
}

describe('dispatchSse — pre-stream errors', () => {
	it('throws ApiError synchronously when the route 503s router_not_configured', async () => {
		mockJsonResponse(
			{ error: 'LLM router not configured', code: 'router_not_configured' },
			{ status: 503 }
		);
		await expect(
			collect({ model: 'm', messages: [{ role: 'user', content: 'hi' }] })
		).rejects.toMatchObject({
			name: 'ApiError',
			status: 503,
			code: 'router_not_configured'
		});
	});

	it('throws ApiError on 400 invalid_body', async () => {
		mockJsonResponse(
			{ error: 'model required', code: 'invalid_body' },
			{ status: 400 }
		);
		await expect(
			collect({ model: '', messages: [] })
		).rejects.toMatchObject({
			name: 'ApiError',
			status: 400,
			code: 'invalid_body'
		});
	});
});

describe('dispatchSse — SSE frame parsing', () => {
	it('yields chunk frames then a terminal done frame', async () => {
		const frames = [
			'event: chunk\ndata: {"delta":"Hel"}\n\n',
			'event: chunk\ndata: {"delta":"lo "}\n\n',
			'event: chunk\ndata: {"delta":"world"}\n\n',
			'event: done\ndata: {"provider":"synthetic","model":"echo","text":"Hello world","usage":{"prompt_tokens":3,"completion_tokens":2},"cache_hit":false,"task_tag":"unit"}\n\n'
		];
		mockSseStream(frames);
		const events = await collect({
			model: 'echo',
			messages: [{ role: 'user', content: 'hi' }],
			task_tag: 'unit'
		});
		expect(events).toHaveLength(4);
		expect(events[0]).toEqual({ kind: 'chunk', delta: 'Hel' });
		expect(events[1]).toEqual({ kind: 'chunk', delta: 'lo ' });
		expect(events[2]).toEqual({ kind: 'chunk', delta: 'world' });
		expect(events[3].kind).toBe('done');
		if (events[3].kind !== 'done') throw new Error('unreachable');
		expect(events[3].payload.provider).toBe('synthetic');
		expect(events[3].payload.usage.prompt_tokens).toBe(3);
		expect(events[3].payload.task_tag).toBe('unit');
	});

	it('yields a terminal error frame on router failure', async () => {
		const frames = [
			'event: error\ndata: {"error":"upstream auth rejected","code":"router_auth"}\n\n'
		];
		mockSseStream(frames);
		const events = await collect({
			model: 'm',
			messages: [{ role: 'user', content: 'x' }]
		});
		expect(events).toHaveLength(1);
		expect(events[0].kind).toBe('error');
		if (events[0].kind !== 'error') throw new Error('unreachable');
		expect(events[0].payload.code).toBe('router_auth');
	});

	it('tolerates frames split across stream chunks (mid-frame TCP boundary)', async () => {
		// Wire-realistic: server flushes mid-data: line, network splits it.
		const frames = [
			'event: chu',
			'nk\ndata: {"de',
			'lta":"abc"}\n',
			'\nevent: done\ndata: {"provider":"s","model":"m","text":"abc","usage":{"prompt_tokens":0,"completion_tokens":0},"cache_hit":false,"task_tag":null}\n\n'
		];
		mockSseStream(frames);
		const events = await collect({
			model: 'm',
			messages: [{ role: 'user', content: 'x' }]
		});
		expect(events).toHaveLength(2);
		expect(events[0]).toEqual({ kind: 'chunk', delta: 'abc' });
		expect(events[1].kind).toBe('done');
	});

	it('ignores SSE comment frames (`: keep-alive`) — they do not yield events', async () => {
		const frames = [
			': keep-alive 1\n\n',
			'event: chunk\ndata: {"delta":"x"}\n\n',
			': keep-alive 2\n\n',
			'event: done\ndata: {"provider":"s","model":"m","text":"x","usage":{"prompt_tokens":0,"completion_tokens":0},"cache_hit":false,"task_tag":null}\n\n'
		];
		mockSseStream(frames);
		const events = await collect({
			model: 'm',
			messages: [{ role: 'user', content: 'x' }]
		});
		// Only chunk + done. Comments are NOT yielded.
		expect(events.map((e) => e.kind)).toEqual(['chunk', 'done']);
	});

	it('ignores unknown event names (forward-compat with new server frame types)', async () => {
		const frames = [
			'event: chunk\ndata: {"delta":"a"}\n\n',
			'event: heartbeat\ndata: {"ts":"now"}\n\n',
			'event: done\ndata: {"provider":"s","model":"m","text":"a","usage":{"prompt_tokens":0,"completion_tokens":0},"cache_hit":false,"task_tag":null}\n\n'
		];
		mockSseStream(frames);
		const events = await collect({
			model: 'm',
			messages: [{ role: 'user', content: 'x' }]
		});
		// `heartbeat` is unknown to the dispatch parser and silently dropped.
		expect(events.map((e) => e.kind)).toEqual(['chunk', 'done']);
	});

	it('throws ApiError when SSE response has no body', async () => {
		// Hand-rolled fetch mock with a null body (Response with no stream).
		globalThis.fetch = vi.fn(() => {
			const r = new Response(null, {
				status: 200,
				headers: { 'content-type': 'text/event-stream' }
			});
			// Some runtimes back-fill an empty stream; force-null for the test.
			Object.defineProperty(r, 'body', { value: null });
			return Promise.resolve(r);
		}) as unknown as typeof fetch;
		await expect(
			collect({ model: 'm', messages: [{ role: 'user', content: 'x' }] })
		).rejects.toMatchObject({ code: 'transport_error' });
	});
});

describe('dispatchSse — request shape', () => {
	it('POSTs JSON with accept: text/event-stream and forwards task_tag verbatim', async () => {
		mockSseStream([
			'event: done\ndata: {"provider":"s","model":"m","text":"","usage":{"prompt_tokens":0,"completion_tokens":0},"cache_hit":false,"task_tag":"alpha"}\n\n'
		]);
		const req: DispatchRequest = {
			model: 'm',
			messages: [
				{ role: 'system', content: 'sys' },
				{ role: 'user', content: 'hi' }
			],
			params: { temperature: 0.2, max_tokens: 100 },
			task_tag: 'alpha'
		};
		await collect(req);
		expect(calls[0].url).toBe('/api/dispatch');
		expect(calls[0].init?.method).toBe('POST');
		const headers = (calls[0].init?.headers ?? {}) as Record<string, string>;
		expect(headers['content-type']).toBe('application/json');
		expect(headers.accept).toBe('text/event-stream');
		expect(JSON.parse(String(calls[0].init?.body))).toEqual(req);
	});
});
