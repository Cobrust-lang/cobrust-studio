/**
 * Typed fetch wrapper around the studio-server REST + SSE surface.
 *
 * All functions are pure — no module-level fetch state, no cookies, no
 * auth header (the M2 auth flow is opaque-blob storage per ADR-0003,
 * not a session token). The dev-mode vite proxy maps `/api/*` to
 * `http://127.0.0.1:7878`; the M3 build embeds same-origin via
 * rust-embed so the same code path works in both modes.
 *
 * Error policy: every non-2xx response is converted into a thrown
 * `ApiError` with the server-supplied `{error, code}` envelope when
 * available, or a synthetic `{error, code: "transport_error"}` for
 * network failures.
 */

import type {
	Adr,
	AdrDraft,
	AdrSummary,
	AgentTurnDone,
	AgentTurnIteration,
	AgentTurnRequest,
	AgentTurnToolCall,
	AgentTurnToolResult,
	DispatchRequest,
	EncryptedBlob,
	EventEnvelope,
	Finding,
	FindingDraft,
	FindingSummary,
	LedgerEntry,
	ModelListResponse,
	ProjectCurrent,
	SessionStatus,
	VersionInfo
} from './types';

/** Thrown by every non-2xx response. Carries the server envelope. */
export class ApiError extends Error {
	readonly status: number;
	readonly code: string;
	constructor(status: number, code: string, message: string) {
		super(message);
		this.status = status;
		this.code = code;
		this.name = 'ApiError';
	}
}

async function parseError(resp: Response): Promise<ApiError> {
	let code = 'unknown_error';
	let message = `HTTP ${resp.status}`;
	try {
		const body = (await resp.json()) as { error?: string; code?: string };
		if (typeof body?.code === 'string') code = body.code;
		if (typeof body?.error === 'string') message = body.error;
	} catch {
		/* non-JSON body — keep defaults */
	}
	return new ApiError(resp.status, code, message);
}

async function jsonGet<T>(path: string): Promise<T> {
	let resp: Response;
	try {
		resp = await fetch(path, { headers: { accept: 'application/json' } });
	} catch (e) {
		throw new ApiError(0, 'transport_error', (e as Error).message);
	}
	if (!resp.ok) throw await parseError(resp);
	return (await resp.json()) as T;
}

async function jsonPost<TReq, TResp>(path: string, body: TReq): Promise<TResp> {
	let resp: Response;
	try {
		resp = await fetch(path, {
			method: 'POST',
			headers: { 'content-type': 'application/json', accept: 'application/json' },
			body: JSON.stringify(body)
		});
	} catch (e) {
		throw new ApiError(0, 'transport_error', (e as Error).message);
	}
	if (!resp.ok) throw await parseError(resp);
	return (await resp.json()) as TResp;
}

// ───── ADR ──────────────────────────────────────────────────────────

export async function listAdrs(): Promise<AdrSummary[]> {
	const body = await jsonGet<{ adrs: AdrSummary[] }>('/api/adr');
	return body.adrs;
}

export async function getAdr(id: number): Promise<Adr> {
	return jsonGet<Adr>(`/api/adr/${id}`);
}

export async function createAdr(draft: AdrDraft): Promise<Adr> {
	return jsonPost<AdrDraft, Adr>('/api/adr', draft);
}

// ───── Finding ──────────────────────────────────────────────────────

export async function listFindings(): Promise<FindingSummary[]> {
	const body = await jsonGet<{ findings: FindingSummary[] }>('/api/finding');
	return body.findings;
}

export async function createFinding(draft: FindingDraft): Promise<Finding> {
	return jsonPost<FindingDraft, Finding>('/api/finding', draft);
}

// ───── Ledger ───────────────────────────────────────────────────────

export async function recentLedger(n = 20): Promise<LedgerEntry[]> {
	const capped = Math.max(0, Math.min(n, 1000));
	const body = await jsonGet<{ entries: LedgerEntry[] }>(`/api/ledger/recent?n=${capped}`);
	return body.entries;
}

// ───── Project / Version ──────────────────────────────────────────

export async function getProject(): Promise<ProjectCurrent> {
	return jsonGet<ProjectCurrent>('/api/project/current');
}

export async function getVersion(): Promise<VersionInfo> {
	return jsonGet<VersionInfo>('/api/version');
}

// ───── Auth (ADR-0003 / ADR-0007 M6) ───────────────────────────────

/**
 * Legacy M2 stub: client-side AES-GCM blob POSTed to the server.
 * Kept exported for the crypto.test.ts test corpus + as a fallback;
 * the live /login page uses `login()` (ADR-0007 server-side AEAD).
 */
export async function setEndpoint(blob: EncryptedBlob): Promise<{ status: string }> {
	return jsonPost<EncryptedBlob, { status: string }>('/api/auth/set-endpoint', blob);
}

/**
 * M6 login (ADR-0007) + M7 multi-provider (ADR-0008). Server derives an
 * Argon2id key from `passphrase`, AEAD-seals `(endpoint, api_key, model,
 * provider_kind)` with AES-256-GCM, and stashes the in-memory `SessionKey`
 * for the run. The plaintext payload travels over TLS/localhost; the server
 * never persists it.
 *
 * `provider_kind` defaults to `"anthropic"` on the server side when omitted
 * (v0.2.x back-compat), but the SvelteKit form always sends it explicitly.
 */
export async function login(payload: {
	endpoint: string;
	api_key: string;
	model: string;
	passphrase: string;
	provider_kind: 'anthropic' | 'openai';
}): Promise<{ status: string }> {
	return jsonPost<typeof payload, { status: string }>('/api/login', payload);
}

export async function logout(): Promise<{ status: string }> {
	return jsonPost<Record<string, never>, { status: string }>('/api/logout', {});
}

export async function getSessionStatus(): Promise<SessionStatus> {
	return jsonGet<SessionStatus>('/api/session/status');
}

export async function previewModels(payload: {
	endpoint: string;
	api_key: string;
	provider_kind: 'anthropic' | 'openai';
}): Promise<ModelListResponse> {
	return jsonPost<typeof payload, ModelListResponse>('/api/models/preview', payload);
}

export async function getSessionModels(): Promise<ModelListResponse> {
	return jsonGet<ModelListResponse>('/api/models/session');
}

// ───── Dispatch (SSE) ──────────────────────────────────────────────

/**
 * Result yielded by `dispatchSse`. One of:
 * - `{ kind: 'chunk', delta }` — append to the running transcript.
 * - `{ kind: 'done', payload }` — final completion with usage + tag.
 * - `{ kind: 'error', payload }` — router-side failure (auth, rate, etc).
 */
export type DispatchEvent =
	| { kind: 'chunk'; delta: string }
	| {
			kind: 'done';
			payload: {
				provider: string;
				model: string;
				text: string;
				usage: { prompt_tokens: number; completion_tokens: number };
				cache_hit: boolean;
				task_tag: string | null;
			};
	  }
	| { kind: 'error'; payload: { error: string; code: string } };

export type AgentTurnEvent =
	| { kind: 'iteration'; payload: AgentTurnIteration }
	| { kind: 'tool_call'; payload: AgentTurnToolCall }
	| { kind: 'tool_result'; payload: AgentTurnToolResult }
	| { kind: 'done'; payload: AgentTurnDone }
	| { kind: 'error'; payload: { error: string; code: string } };

/**
 * Stream `POST /api/dispatch` as an async iterable of typed events.
 *
 * Pre-stream errors (router not configured, malformed body) surface as a
 * synchronous throw of `ApiError` before the first iteration; once the
 * stream begins, router-side failures arrive as `{kind: 'error'}` events.
 *
 * The browser `EventSource` API doesn't accept `POST`, so this is a
 * hand-rolled SSE parser over `fetch` + `ReadableStream`. The server
 * emits `\n\n`-delimited frames with `event:` and `data:` lines.
 *
 * Cancellation: pass `AbortSignal` via `signal` to drop the connection.
 */
export async function* dispatchSse(
	req: DispatchRequest,
	signal?: AbortSignal
): AsyncGenerator<DispatchEvent, void, void> {
	yield* postSse('/api/dispatch', req, parseDispatchSseFrame, signal);
}

export async function* agentTurnSse(
	req: AgentTurnRequest,
	signal?: AbortSignal
): AsyncGenerator<AgentTurnEvent, void, void> {
	yield* postSse('/api/agent-turn', req, parseAgentTurnSseFrame, signal);
}

async function* postSse<TReq, TEvent>(
	path: string,
	req: TReq,
	parseFrame: (frame: string) => TEvent | null,
	signal?: AbortSignal
): AsyncGenerator<TEvent, void, void> {
	let resp: Response;
	try {
		resp = await fetch(path, {
			method: 'POST',
			headers: { 'content-type': 'application/json', accept: 'text/event-stream' },
			body: JSON.stringify(req),
			signal
		});
	} catch (e) {
		throw new ApiError(0, 'transport_error', (e as Error).message);
	}
	if (!resp.ok) throw await parseError(resp);
	if (!resp.body) throw new ApiError(resp.status, 'transport_error', 'no SSE body');

	const reader = resp.body.getReader();
	const decoder = new TextDecoder();
	let buffer = '';
	try {
		while (true) {
			const { value, done } = await reader.read();
			if (done) break;
			buffer += decoder.decode(value, { stream: true });
			// SSE frames are blank-line delimited.
			let idx = buffer.indexOf('\n\n');
			while (idx !== -1) {
				const frame = buffer.slice(0, idx);
				buffer = buffer.slice(idx + 2);
				const event = parseFrame(frame);
				if (event) yield event;
				idx = buffer.indexOf('\n\n');
			}
		}
	} finally {
		try {
			reader.releaseLock();
		} catch {
			/* ignore */
		}
	}
}

/**
 * Parse one SSE frame (already split on `\n\n`). Lines beginning with
 * `:` are comments (keep-alive frames) and yield `null`. Returns the
 * typed `DispatchEvent` on success.
 */
function parseDispatchSseFrame(frame: string): DispatchEvent | null {
	const parsed = parseRawSseFrame(frame);
	if (!parsed) return null;
	switch (parsed.event) {
		case 'chunk':
			return { kind: 'chunk', delta: String(parsed.payload.delta ?? '') };
		case 'done':
			return { kind: 'done', payload: parsed.payload };
		case 'error':
			return { kind: 'error', payload: parsed.payload };
		default:
			return null;
	}
}

function parseAgentTurnSseFrame(frame: string): AgentTurnEvent | null {
	const parsed = parseRawSseFrame(frame);
	if (!parsed) return null;
	switch (parsed.event) {
		case 'iteration':
			return { kind: 'iteration', payload: parsed.payload };
		case 'tool_call':
			return { kind: 'tool_call', payload: parsed.payload };
		case 'tool_result':
			return { kind: 'tool_result', payload: parsed.payload };
		case 'done':
			return { kind: 'done', payload: parsed.payload };
		case 'error':
			return { kind: 'error', payload: parsed.payload };
		default:
			return null;
	}
}

function parseRawSseFrame(frame: string): { event: string; payload: any } | null {
	let event = 'message';
	const data: string[] = [];
	for (const rawLine of frame.split('\n')) {
		const line = rawLine.replace(/\r$/, '');
		if (line === '' || line.startsWith(':')) continue;
		if (line.startsWith('event:')) event = line.slice(6).trim();
		else if (line.startsWith('data:')) data.push(line.slice(5).trimStart());
	}
	if (data.length === 0) return null;
	try {
		return { event, payload: JSON.parse(data.join('\n')) };
	} catch {
		return null;
	}
}

// ───── Event stream subscription (`GET /api/events`) ──────────────

/**
 * Subscribe to `/api/events` via the browser EventSource. Returns a
 * disposer; call it to close the stream.
 *
 * `onEvent` fires for every typed envelope; transport keep-alives
 * (`:keep-alive` comment frames) are silently swallowed by the
 * browser's EventSource parser, which is exactly what we want — they
 * exist solely so the connection doesn't go idle past the proxy
 * timeout.
 */
export function subscribeEvents(
	onEvent: (e: EventEnvelope) => void,
	onError?: (e: Event) => void
): () => void {
	const es = new EventSource('/api/events');
	const handler = (kind: EventEnvelope['kind']) => (ev: MessageEvent) => {
		try {
			const parsed = JSON.parse(ev.data) as Partial<EventEnvelope>;
			// The Rust side already sets the `kind` field via #[serde(tag=...)],
			// but we re-stamp here for safety in case a future envelope omits it.
			onEvent({ ...(parsed as EventEnvelope), kind } as EventEnvelope);
		} catch {
			/* ignore unparseable */
		}
	};
	const kinds: EventEnvelope['kind'][] = [
		'adr_added',
		'adr_modified',
		'adr_removed',
		'finding_added',
		'finding_modified',
		'finding_removed'
	];
	for (const k of kinds) es.addEventListener(k, handler(k));
	if (onError) es.onerror = onError;
	return () => {
		es.close();
	};
}
