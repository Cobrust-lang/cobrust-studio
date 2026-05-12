---
adr_id: "0008"
title: M7 multi-provider /login — Anthropic + OpenAI-compatible via the SvelteKit form
status: accepted
date: 2026-05-12
supersedes: []
superseded_by: []
---

# ADR-0008: multi-provider /login (M7)

## Context

v0.2.0 shipped the M6 AEAD round-trip per ADR-0007. The current
dispatch flow:

1. SvelteKit `/login` page POSTs `{endpoint, api_key, model,
   passphrase}` to `/api/login`.
2. Server derives Argon2id key, seals the `EndpointSecret`, stashes
   the `SessionKey` in `AppState`.
3. On dispatch, `resolve_router()` decrypts the blob and builds an
   `AnthropicProvider` (`crates/studio-server/src/routes/dispatch.rs`).

Step 3 hardcodes `AnthropicProvider`. The form-driven path therefore
only works for Anthropic-API-compatible endpoints (canonical
`https://api.anthropic.com` plus drop-in compat shims).

Sarah v3 audit finding #3 surfaced this as a v0.3.x blocker:

> "Multi-provider parallel dispatch. Studio is effectively
> Anthropic-only through the session path... For teams running
> OpenAI or local Ollama, this is a blocker. The fix is adding a
> `provider_kind` field to `LoginRequest`..."

Confirmed by Sarah v4 review of the v0.2.1 cross-platform-stable
posture (separate report).

The `studio.toml` static-router path already supports OpenAI-compat
(via `[providers.X] kind = "openai"`) — that's the env-var fallback
the README documents. Multi-provider exists in the backend; the
gap is that `/login` doesn't expose it.

Hard constraints carried from prior ADRs:

- **ADR-0007 §"Architecture pin"** — server-side derive, in-memory
  `SessionKey`, server holds the plaintext API key briefly. M7 keeps
  this.
- **ADR-0007 §"Storage wire format"** — `salt(16) || nonce(12) ||
  ciphertext+tag` packed under `scheme = "aes-gcm-256/argon2id-v1"`.
  M7 inherits; `provider_kind` lives **inside** the encrypted payload,
  not in the SQLite row's plaintext metadata.
- **ADR-0005 / ADR-0006** — studio-router exposes `AnthropicProvider`
  and `OpenAiProvider`. Both implement `LlmProvider`. M7 reuses both.

## Threat model (additions)

- **Provider-kind tampering**: an attacker with disk-write access
  could not flip `provider_kind` from `anthropic` to `openai` because
  the field lives inside the AEAD ciphertext. Tampering invalidates
  the GCM tag.
- **Endpoint URL phishing**: not new to M7 — a user typing
  `https://anthropic.evil.example.com` into /login already gets owned
  in M6. M7 does not change this.

## Options considered

### Option A — Explicit `provider_kind` field on `LoginRequest`

`/api/login` accepts:

```json
{
  "endpoint": "...",
  "api_key": "...",
  "model": "...",
  "passphrase": "...",
  "provider_kind": "anthropic"  // or "openai"
}
```

The SvelteKit form adds a `<select>` with two options. URL-based
auto-suggest (see §"UX hint" below) preselects the right one.

**Pros**:
- Explicit + future-extensible (add `"vllm"`, `"ollama"`, `"groq"`
  as needed without ambiguity).
- The EndpointSecret stores the kind alongside the credential, so
  re-login + dispatch + restart all use the same value.
- Matches the existing `studio.toml::ProviderKind` enum semantics.

**Cons**:
- Adds a required field to the public POST body — a small wire-
  format change, but `/api/login` itself only shipped in v0.2.0, so
  semver-wise this is a v0.3.x minor bump (additive change, default
  to `"anthropic"` if missing to keep old curl-based tests working).

### Option B — Auto-detect from URL pattern

Server greps the endpoint URL: contains `anthropic.com` →
AnthropicProvider; otherwise OpenAiProvider.

**Pros**: zero wire-format change.

**Cons**: brittle (`anthropic-proxy.internal.example.com` would be
misclassified; OpenAI-compat shims that proxy Anthropic API would be
double-mismatched). Auto-detect is reasonable as a FALLBACK but
shouldn't be the source of truth.

### Option C — Both: explicit field + URL hint in UI

LoginRequest accepts an explicit `provider_kind`; the SvelteKit form
auto-suggests the value based on URL when the user types. User can
override the suggestion. Server uses the submitted value, never the
URL.

**Pros**: best of both. Wire format is unambiguous; UX is friendly.

**Cons**: ~50 LoC more in the SvelteKit form than Option A.

### Option D — Per-provider routes: `POST /api/login/anthropic` + `POST /api/login/openai`

Two routes; provider_kind encoded in the URL.

**Pros**: URL = type. Easy to docs.

**Cons**: rejects the "single login endpoint" REST idiom Studio has
been building toward. The SvelteKit form would have to choose-URL-
before-posting which is the same UX work as Option A. Forecloses
future variants (e.g. `oauth-anthropic`) without proliferating
endpoints.

## Decision

**Option C** — explicit `provider_kind` field + URL-based hint in UI.

### Wire-format change (additive)

`LoginRequest` JSON schema in `crates/studio-server/src/routes/login.rs`:

```rust
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub passphrase: String,
    #[serde(default)]  // defaults to ProviderKind::Anthropic for v0.2.x compat
    pub provider_kind: ProviderKind,
}
```

`ProviderKind` already exists in `studio-router::config`:

```rust
pub enum ProviderKind { Anthropic, Openai, Synthetic }
```

`Synthetic` is not a valid `/login` value (CLI-only, dev). Reject at
the route with `400 {code: "invalid_provider_kind"}`.

### EndpointSecret extension

```rust
#[derive(Serialize, Deserialize)]
pub struct EndpointSecret {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]  // defaults to Anthropic for blobs sealed pre-M7
    pub provider_kind: ProviderKind,
}
```

Backward compat: blobs sealed by v0.2.x (no `provider_kind` field)
deserialize with `provider_kind = Anthropic`, matching the v0.2.x
implicit behavior.

### Dispatch integration

In `crates/studio-server/src/routes/dispatch.rs::resolve_router()`,
the per-request provider construction becomes:

```rust
let provider: Arc<dyn LlmProvider> = match secret.provider_kind {
    ProviderKind::Anthropic => Arc::new(
        AnthropicProvider::new(name, secret.endpoint, secret.api_key)?
    ),
    ProviderKind::Openai => Arc::new(
        OpenAiProvider::new(name, secret.endpoint, secret.api_key)?
    ),
    ProviderKind::Synthetic => {
        return Err(StudioError::InvalidSession(
            "synthetic provider not valid for session-driven dispatch"
        ));
    }
};
```

### SvelteKit form UX

Add a `<select>` field labeled "Provider":

```svelte
<label>
  <span>Provider</span>
  <select bind:value={providerKind}>
    <option value="anthropic">Anthropic API</option>
    <option value="openai">OpenAI-compatible (vLLM / DeepSeek / Together / OpenRouter / Groq / Ollama)</option>
  </select>
</label>
```

URL-based hint logic:

```typescript
$: {
  if (baseUrl.includes('anthropic.com')) providerKind = 'anthropic';
  else if (baseUrl.length > 0) providerKind = 'openai';
}
```

Reactive: typing the URL updates the dropdown, but the user can
override.

### `--dev-api-key` CLI flag extension

`--dev-provider-kind <KIND>` (default `anthropic` for backward compat
with v0.2.x users who set `--dev-api-key`):

```
cobrust-studio serve \
  --dev-api-key sk-... \
  --dev-endpoint https://api.openai.com/v1 \
  --dev-model gpt-4o \
  --dev-provider-kind openai
```

## Done means (falsifiable success criteria)

1. **Unit tests pass** (`cargo test -p studio-server secret::`):
   - `endpoint_secret_back_compat` — deserialize a v0.2.x JSON
     payload (no `provider_kind` field) → `provider_kind = Anthropic`.
   - `endpoint_secret_forward_compat` — serialize an
     `EndpointSecret { provider_kind: Openai }` → deserialize →
     round-trip preserves the kind.

2. **Integration tests pass** (`crates/studio-server/tests/
   multi_provider_login.rs`, NEW):
   - `login_anthropic_then_dispatch` — POST /api/login with
     `provider_kind: "anthropic"` → wiremock Anthropic stub → dispatch
     returns 200.
   - `login_openai_then_dispatch` — POST /api/login with
     `provider_kind: "openai"` → wiremock OpenAI stub → dispatch
     returns 200.
   - `login_synthetic_returns_400` — `provider_kind: "synthetic"` → 400
     `{code: "invalid_provider_kind"}`.
   - `login_missing_provider_kind_defaults_anthropic` — body without
     the field → 200 (back-compat).
   - `re_login_changes_provider_kind` — first login anthropic, second
     same passphrase + openai → both succeed (wrong-passphrase guard
     verifies the PASSPHRASE only, not the kind; rotation of
     provider_kind is a legitimate user action).
   - `existing_blob_decryption_supplies_kind` — pre-M7 blob (no
     `provider_kind` field) → decrypt → defaults to Anthropic.

3. **E2E spec passes** (`web/tests/e2e/login-multi-provider.spec.ts`,
   NEW):
   - Hermetic Playwright launches the binary without env vars →
     visits /login → fills endpoint=https://example.invalid +
     selects OpenAI-compatible from the dropdown → submits → asserts
     the next page renders.
   - URL-based hint: type `https://api.anthropic.com` → dropdown
     auto-selects "Anthropic API"; type
     `https://api.deepseek.com/v1` → auto-selects "OpenAI-compatible".

4. **Doc-coverage 7-gate stays green**.

5. **Module-doc + zh/en human-track docs updated**:
   - `docs/agent/modules/studio-server.md` — `secret` submodule
     enumeration includes the `provider_kind` field.
   - `docs/human/{zh,en}/secret-storage.md` — describes the provider
     selection UX.

6. **CHANGELOG entry** for v0.3.0:
   - References ADR-0008 + closes Sarah v3 #3.

7. **README update**:
   - "Looking for design partners" priority list — gate #3
     (multi-provider /login) crossed off.

## Phase plan (per ADSD §"Two-phase dispatch SOP")

**Phase 1 (this commit, CTO solo)**:
- This ADR landed.
- Optional: test-skeleton `crates/studio-server/tests/
  multi_provider_login.rs` with `#[ignore]`-attributed placeholders
  so Phase 2 P9 has a clear test target.

**Phase 2 (P9 dispatch, ~60-90 min wall-clock)**:
- Worktree: `feature/m7-multi-provider-login`.
- Deliverables: `LoginRequest` field + `EndpointSecret` field +
  dispatch match arm + SvelteKit form selector + URL-hint reactive
  logic + 2 unit + 6 integration tests + 1 E2E + 2 doc updates +
  README + CHANGELOG + `--dev-provider-kind` CLI flag.
- 7-gate green; CTO 守闸; merge `--no-ff`.

## Consequences

- **Enables**: Sarah v3 / v4 audit gate #3 closes → pilot-readiness
  for teams running OpenAI-compat (vLLM, DeepSeek, Together,
  OpenRouter, Groq, local Ollama via `/v1/chat/completions` shim).
- **Enables**: removing the README's "Multi-provider /login" priority
  item from the design-partner blocker list.
- **Forecloses**: per-provider routes (Option D) for v0.3.x. Reopens
  if oauth-anthropic-vs-oauth-google etc. land — at that point a
  proper `/api/auth/{provider}` family makes sense.
- **Migration**: v0.2.x → v0.3.0 wire format is additive only.
  Pre-M7 sealed blobs deserialize to `provider_kind = Anthropic`
  matching the v0.2.x implicit behavior. No on-disk migration step
  required.
- **Performance**: zero. The new field adds ~16 bytes to the JSON
  payload pre-seal; AEAD ciphertext grows by the same. Argon2id
  derivation cost unchanged.

## Cross-references

- ADR-0001 (stack — async tokio, Rust 2024 edition)
- ADR-0003 (auth — custom-endpoint-first; M7 keeps the form-driven
  posture)
- ADR-0005 / ADR-0006 (studio-router public surface — provides
  `AnthropicProvider` + `OpenAiProvider`, both `LlmProvider` impls)
- ADR-0007 (M6 AEAD round-trip — M7 extends `LoginRequest` +
  `EndpointSecret` additively)
- src: `crates/studio-server/src/routes/login.rs` (extend
  LoginRequest)
- src: `crates/studio-server/src/secret.rs` (extend EndpointSecret)
- src: `crates/studio-server/src/routes/dispatch.rs::resolve_router`
  (match arm on provider_kind)
- src: `crates/studio-router/src/openai.rs::OpenAiProvider`
  (the OpenAI-compat impl already exists from the lift)
- src: `web/src/routes/login/+page.svelte` (add Provider dropdown +
  URL hint)
- Sarah v3 / v4 audit (pilot-gate #3 driver)
