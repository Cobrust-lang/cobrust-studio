/**
 * TypeScript mirrors of `studio-server` / `studio-store` / `studio-router`
 * wire shapes. These are the **post-A5-reconcile** shapes — `Adr` and
 * `Finding` are flat on the wire (the Rust side carries
 * `#[serde(flatten)]` on the embedded summary struct), so the TS types
 * reflect that directly.
 *
 * Source of truth (binding):
 * - `crates/studio-store/src/adr.rs` (Adr, AdrSummary, AdrDraft)
 * - `crates/studio-store/src/finding.rs` (Finding, FindingSummary, FindingDraft)
 * - `crates/studio-server/src/routes/*.rs` (request/response envelopes)
 * - `crates/studio-router/src/ledger.rs` (LedgerEntry, Outcome)
 * - `crates/studio-router/src/provider.rs` (TokenUsage, SamplingParams)
 *
 * Per ADR-0006 §"Addendum 2026-05-11" F-03 the dispatch request carries
 * an optional `task_tag` that round-trips into the SSE `done` payload.
 */

// ───── ADR ──────────────────────────────────────────────────────────

/** Summary projection — list-view row. Wire shape per `AdrSummary`. */
export interface AdrSummary {
	adr_id: number;
	title: string;
	status: string;
	date: string;
	/** Absolute path on disk (display string). */
	path: string;
}

/**
 * Full ADR. Note the **flattened** wire shape: `adr_id`, `title`, `status`,
 * `date`, `path` live at the top level (not nested under `summary`). This
 * matches the A5-reconcile `#[serde(flatten)]` on the Rust side.
 */
export interface Adr extends AdrSummary {
	/** Markdown body after the YAML frontmatter. */
	body: string;
	supersedes: number[];
	superseded_by: number[];
}

/** POST /api/adr request body. */
export interface AdrDraft {
	title: string;
	/** Defaults to `"proposed"` server-side when omitted/empty. */
	status?: string;
	/** ISO date `YYYY-MM-DD`; defaults to today UTC server-side. */
	date?: string;
	body?: string;
	supersedes?: number[];
}

// ───── Finding ──────────────────────────────────────────────────────

/** Summary projection — list-view row. */
export interface FindingSummary {
	finding_id: string;
	title: string;
	status: string;
	severity: string;
	/** `last_verified_commit` or a date — caller renders as-is. */
	date: string;
	path: string;
}

/** Full Finding. Flattened wire shape, per A5 reconcile. */
export interface Finding extends FindingSummary {
	body: string;
	dependencies: string[];
	related: string[];
}

/** POST /api/finding request body. */
export interface FindingDraft {
	finding_id: string;
	title: string;
	/** Defaults to `"HEAD"` server-side (F20 gate enforces real SHA). */
	last_verified_commit?: string;
	/** Defaults to `"P3"`. */
	severity?: string;
	/** Defaults to `"open"`. */
	status?: string;
	dependencies?: string[];
	related?: string[];
	body?: string;
}

// ───── Ledger ───────────────────────────────────────────────────────

export type Outcome = 'ok' | 'error_transient' | 'error_permanent';
export type ProviderKind = 'anthropic' | 'openai' | 'synthetic';

export interface LedgerEntry {
	ts: string;
	task_tag: string | null;
	provider: string;
	provider_kind: ProviderKind | null;
	model: string;
	cache_key: string;
	cache_hit: boolean;
	prompt_tokens: number;
	completion_tokens: number;
	total_tokens: number;
	latency_ms: number;
	attempt: number;
	outcome: Outcome;
	error_code: string | null;
}

// ───── Auth (ADR-0003) ─────────────────────────────────────────────

/**
 * Opaque AEAD-encrypted credential triple. Server is a pass-through —
 * it persists the triple under the `"endpoint"` slot without decryption.
 *
 * Real AEAD round-trip arrives at M3 (the M2 client-side WebCrypto stub
 * uses AES-GCM-256 with a session-derived key — see
 * `src/lib/crypto.ts`).
 */
export interface EncryptedBlob {
	/** Base64-encoded ciphertext bytes. */
	ciphertext: string;
	/** Base64-encoded nonce / IV bytes. */
	nonce: string;
	/** Scheme tag, e.g. `"aes-gcm-256/m2-stub"`. */
	scheme: string;
}

// ───── Project / Version ───────────────────────────────────────────

export interface ProjectCurrent {
	project_root: string;
	/** RFC-3339 UTC timestamp captured at startup. */
	started_at: string;
	version: string;
}

export interface VersionInfo {
	studio_server: string;
	studio_store: string;
	studio_router: string;
	rustc: string;
}

// ───── Dispatch (ADR-0006 §F-03) ──────────────────────────────────

export type DispatchRole = 'system' | 'user' | 'assistant';

export interface DispatchMessage {
	role: DispatchRole;
	content: string;
}

export interface SamplingParams {
	max_tokens?: number;
	temperature?: number;
	top_p?: number;
	stop?: string[];
}

export interface DispatchRequest {
	model: string;
	messages: DispatchMessage[];
	params?: SamplingParams;
	/** Caller-supplied tag — echoed into the SSE `done` payload (F-03). */
	task_tag?: string;
}

export interface TokenUsage {
	prompt_tokens: number;
	completion_tokens: number;
}

/** Payload of the SSE `event: done` frame. */
export interface DispatchDone {
	provider: string;
	model: string;
	text: string;
	usage: TokenUsage;
	cache_hit: boolean;
	task_tag: string | null;
}

/** Payload of the SSE `event: chunk` frame. */
export interface DispatchChunk {
	delta: string;
}

/** Payload of any error envelope (route or SSE `error` event). */
export interface ErrorEnvelope {
	error: string;
	code: string;
}

// ───── SSE state-change events (`GET /api/events`) ────────────────

export type EventEnvelope =
	| { kind: 'adr_added'; path: string }
	| { kind: 'adr_modified'; path: string }
	| { kind: 'adr_removed'; path: string }
	| { kind: 'finding_added'; path: string }
	| { kind: 'finding_modified'; path: string }
	| { kind: 'finding_removed'; path: string }
	| { kind: 'heartbeat' };
