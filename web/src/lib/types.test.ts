/**
 * Compile-time-only TypeScript shape tests for the M2 wire contract.
 *
 * These tests don't assert runtime behaviour — they exist so that
 * `pnpm run check` (svelte-check) fails if the Rust serde shapes drift
 * away from `src/lib/types.ts` and someone forgets to update both
 * sides. The fixtures intentionally include EVERY field declared by
 * the corresponding interface so an `--noImplicitAny`-style accidental
 * `any` doesn't paper over a missing column.
 *
 * Runtime checks: a single trivial `expect(true).toBe(true)` keeps
 * Vitest happy with a non-empty test file.
 *
 * Anchor: docs/agent/modules/web-frontend.md §"Wire contract".
 */

import { describe, expect, it } from 'vitest';
import type {
	Adr,
	AdrDraft,
	AdrSummary,
	DispatchDone,
	DispatchMessage,
	DispatchRequest,
	EncryptedBlob,
	ErrorEnvelope,
	EventEnvelope,
	Finding,
	FindingDraft,
	FindingSummary,
	LedgerEntry,
	Outcome,
	ProjectCurrent,
	ProviderKind,
	SamplingParams,
	TokenUsage,
	VersionInfo
} from './types';

// ───── Fixtures (compile-time pinning) ──────────────────────────────

const _adrSummary: AdrSummary = {
	adr_id: 1,
	title: 'Adopt SvelteKit',
	status: 'accepted',
	date: '2026-05-01',
	path: '/p/adr/0001.md'
};

/** Verifies the **flat** Adr shape — adr_id/title/status/date/path at top level. */
const _adr: Adr = {
	adr_id: 2,
	title: 'Use SSE for dispatch',
	status: 'accepted',
	date: '2026-05-10',
	path: '/p/adr/0002.md',
	body: '## Decision\n…',
	supersedes: [],
	superseded_by: [3]
};

const _adrDraftFull: AdrDraft = {
	title: 'Draft title',
	status: 'proposed',
	date: '2026-05-11',
	body: '## Body',
	supersedes: [1, 2]
};

/** Minimal AdrDraft — all but `title` are optional with server-side defaults. */
const _adrDraftMin: AdrDraft = { title: 'Minimal' };

const _findingSummary: FindingSummary = {
	finding_id: 'f-1',
	title: 'leak',
	status: 'open',
	severity: 'P2',
	date: 'abc123',
	path: '/p/findings/f-1.md'
};

const _finding: Finding = {
	finding_id: 'f-2',
	title: 'subtle bug',
	status: 'closed_by_m2',
	severity: 'P1',
	date: 'def456',
	path: '/p/findings/f-2.md',
	body: '## Repro\n…',
	dependencies: ['adr:0006'],
	related: []
};

const _findingDraftMin: FindingDraft = {
	finding_id: 'f-3',
	title: 'min'
};

const _findingDraftFull: FindingDraft = {
	finding_id: 'f-4',
	title: 'full',
	last_verified_commit: 'HEAD',
	severity: 'P3',
	status: 'open',
	dependencies: ['adr:0001'],
	related: ['finding:f-1'],
	body: '## body'
};

// Pin the union literals — fail if anyone reshapes the enum.
const _outcomeOk: Outcome = 'ok';
const _outcomeT: Outcome = 'error_transient';
const _outcomeP: Outcome = 'error_permanent';

const _provKindAnthropic: ProviderKind = 'anthropic';
const _provKindOpenAI: ProviderKind = 'openai';
const _provKindSyn: ProviderKind = 'synthetic';

const _ledger: LedgerEntry = {
	ts: '2026-05-11T00:00:00Z',
	task_tag: 'agent-turn',
	provider: 'synthetic',
	provider_kind: 'synthetic',
	model: 'echo',
	cache_key: 'abc',
	cache_hit: false,
	prompt_tokens: 10,
	completion_tokens: 5,
	total_tokens: 15,
	latency_ms: 42,
	attempt: 1,
	outcome: 'ok',
	error_code: null
};

const _ledgerErr: LedgerEntry = {
	ts: '2026-05-11T00:00:01Z',
	task_tag: null,
	provider: 'anthropic',
	provider_kind: null,
	model: 'claude-opus-4-7',
	cache_key: 'def',
	cache_hit: true,
	prompt_tokens: 1,
	completion_tokens: 0,
	total_tokens: 1,
	latency_ms: 8,
	attempt: 2,
	outcome: 'error_transient',
	error_code: 'router_rate_limit'
};

const _blob: EncryptedBlob = {
	ciphertext: 'AAAA',
	nonce: 'BBBB',
	scheme: 'aes-gcm-256/m2-stub'
};

const _project: ProjectCurrent = {
	project_root: '/tmp/x',
	started_at: '2026-05-11T00:00:00Z',
	version: '0.1.0'
};

const _version: VersionInfo = {
	studio_server: '0.1.0',
	studio_store: '0.1.0',
	studio_router: '0.1.0',
	rustc: '1.85.0'
};

const _msg: DispatchMessage = { role: 'user', content: 'hi' };
const _msgSys: DispatchMessage = { role: 'system', content: 'sys' };
const _msgAsst: DispatchMessage = { role: 'assistant', content: 'response' };

const _params: SamplingParams = {
	max_tokens: 1024,
	temperature: 0.2,
	top_p: 0.95,
	stop: ['\n\n']
};

const _dispatchReq: DispatchRequest = {
	model: 'claude-opus-4-7',
	messages: [_msgSys, _msg],
	params: _params,
	task_tag: 'agent-turn'
};

/** Minimal dispatch request — `params` + `task_tag` both optional. */
const _dispatchReqMin: DispatchRequest = {
	model: 'm',
	messages: [{ role: 'user', content: 'x' }]
};

const _usage: TokenUsage = { prompt_tokens: 1, completion_tokens: 2 };

const _done: DispatchDone = {
	provider: 'synthetic',
	model: 'echo',
	text: 'hello',
	usage: _usage,
	cache_hit: false,
	task_tag: 'agent-turn'
};

const _doneNullTag: DispatchDone = {
	provider: 's',
	model: 'm',
	text: '',
	usage: { prompt_tokens: 0, completion_tokens: 0 },
	cache_hit: true,
	task_tag: null
};

const _errEnvelope: ErrorEnvelope = {
	error: 'auth rejected',
	code: 'router_auth'
};

// Exhaustive coverage of every `EventEnvelope` variant.
const _evtAdded: EventEnvelope = { kind: 'adr_added', path: '/p/0001.md' };
const _evtMod: EventEnvelope = { kind: 'adr_modified', path: '/p/0001.md' };
const _evtRem: EventEnvelope = { kind: 'adr_removed', path: '/p/0001.md' };
const _evtFAdded: EventEnvelope = { kind: 'finding_added', path: '/p/f.md' };
const _evtFMod: EventEnvelope = { kind: 'finding_modified', path: '/p/f.md' };
const _evtFRem: EventEnvelope = { kind: 'finding_removed', path: '/p/f.md' };
const _evtHb: EventEnvelope = { kind: 'heartbeat' };

// Silence unused-variable diagnostics — the fixtures exist only for
// their type assignability, not for runtime use.
void [
	_adrSummary,
	_adr,
	_adrDraftFull,
	_adrDraftMin,
	_findingSummary,
	_finding,
	_findingDraftMin,
	_findingDraftFull,
	_outcomeOk,
	_outcomeT,
	_outcomeP,
	_provKindAnthropic,
	_provKindOpenAI,
	_provKindSyn,
	_ledger,
	_ledgerErr,
	_blob,
	_project,
	_version,
	_msg,
	_msgSys,
	_msgAsst,
	_params,
	_dispatchReq,
	_dispatchReqMin,
	_usage,
	_done,
	_doneNullTag,
	_errEnvelope,
	_evtAdded,
	_evtMod,
	_evtRem,
	_evtFAdded,
	_evtFMod,
	_evtFRem,
	_evtHb
];

describe('types compile-check', () => {
	it('fixtures above passed tsc — runtime is a no-op', () => {
		expect(true).toBe(true);
	});

	it('Adr.adr_id is a flat top-level number (not nested under summary)', () => {
		// Runtime smoke test — guarantees the flat shape isn't lost to a
		// refactor that nests it back under `.summary`.
		expect(typeof _adr.adr_id).toBe('number');
		expect(typeof _adr.body).toBe('string');
	});

	it('Finding.finding_id is a flat top-level string', () => {
		expect(typeof _finding.finding_id).toBe('string');
		expect(Array.isArray(_finding.dependencies)).toBe(true);
	});

	it('LedgerEntry.outcome is one of the three literal strings', () => {
		const valid: Outcome[] = ['ok', 'error_transient', 'error_permanent'];
		expect(valid).toContain(_ledger.outcome);
	});
});
