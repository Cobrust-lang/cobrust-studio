---
adr_id: "0007"
title: M6 secret-storage — AEAD round-trip for /login → dispatch
status: accepted
date: 2026-05-12
supersedes: []
superseded_by: []
---

# ADR-0007: secret-storage AEAD round-trip (M6)

## Context

v0.1.3 ships with a working `/login` SvelteKit page and a
`session_kv` SQLite table that stores `(ciphertext, nonce, scheme)`
triples — but **no actual encryption layer**. The wire path between
"user enters API key in /login" and "studio-router dispatches with
that key" is currently:

1. `/login` page accepts `(endpoint, api_key, passphrase)` →
   WebCrypto-encrypts client-side → POSTs to a stub endpoint.
2. Studio-server stub writes the opaque blob to `session_kv` under
   slot `"endpoint"` (per `crates/studio-store/src/session.rs:18`).
3. On dispatch, `studio-router::AnthropicProvider::new()` is
   constructed with `api_key` resolved from **environment variable**
   (per `crates/studio-router/src/config.rs:55` field
   `api_key_env: String` and `anthropic.rs:30`-style direct String).
4. The encrypted blob in `session_kv` is **never read**.

This is the design Sarah persona v2 called out as pilot-gate #2:
> "AEAD round-trip ships, env-var workaround removed."

Per `studio-store/src/session.rs:1-7`:
> studio-store doesn't perform the encryption itself (that's the job
> of studio-server's auth layer, which holds the user's passphrase
> or per-session key); we store the opaque `(ciphertext, nonce,
> scheme)` triple keyed by a string slot id.

The architectural invariant is preserved: **studio-store does not
decrypt**. M6 introduces the crypto layer in studio-server that
closes the round-trip.

ADR-0003 §"Decision" already binds the auth model:
> Login screen has two tabs: "API key" tab: `base_url` text field +
> `api_key` password field + `model` text field. Stored
> client-side, encrypted via WebCrypto.

ADR-0003 was silent on where the *server-side* key derivation /
decryption lives because the M3-M4 milestones did not need it
(env-var fallback worked for dogfood). M6 makes that decision.

## Hard constraints carried from prior ADRs

- ADR-0001: Rust 2024 edition, `tokio` async, no `.unwrap()` in
  non-test lib code, no allocation in hot paths if avoidable.
- ADR-0003: API keys never on disk in plaintext (client-side encrypt
  before POST is the prior intent; this ADR may revise to
  server-side derive after passphrase POST — see §"Options" below).
- ADR-0004: SQLite is the index, filesystem is the source of truth.
  session_kv lives in SQLite; that's the only persistent surface.
- ADR-0006 / F-02 addendum: studio-router has zero dep on
  studio-store. Crypto layer must not pull store as a router dep.

## Threat model (in scope)

1. **Cold disk theft.** Attacker steals the `.cobrust-studio/db`
   file. Without the passphrase the blob is opaque. **In scope.**
2. **Stop-and-restart attack.** Attacker has filesystem read and
   waits for the user to type their passphrase at /login, then
   reads it from the wire. **Out of scope** — TLS termination is
   the user's responsibility (Studio docs recommend
   reverse-proxy-with-cert or `127.0.0.1`-only mode per ADR-0003
   Option 3 fallback).
3. **Running-process memory dump.** Attacker with `ptrace` /
   coredump access reads the in-memory decrypted key. **Out of
   scope** — same OS user as the binary trivially defeats any
   in-process crypto. Single-user-MVP constraint per CLAUDE.md §1.
4. **Passphrase phishing / shoulder-surfing.** Out of scope —
   user-discipline problem, not a crypto problem.
5. **Multi-user / per-tenant key separation.** Out of scope per
   CLAUDE.md §1 ("Single-user / single-project / no RBAC").

## Algorithm choice

| Layer | Choice | Why |
|---|---|---|
| Symmetric AEAD | **AES-256-GCM** | Hardware-accelerated on aarch64 + x86_64 (the 5 platforms we ship); IETF standard; the `aes-gcm` RustCrypto crate is audited; `chacha20poly1305` is the alternative but offers no advantage on aarch64-server / x86_64-server targets. |
| KDF | **Argon2id (RFC 9106)** | OWASP password-storage 2025 recommendation; `argon2` RustCrypto crate is pure-Rust + audited. Parameters: `m=64 MiB, t=3, p=1` (OWASP minimum for interactive auth). Calibrate to ~500 ms wall-clock at login on a 2020-era laptop — acceptable for one-time login, prohibitive for online attack. |
| Nonce | **96-bit (12-byte) random per encryption** | AES-GCM standard; collision risk negligible for the volume Studio writes (≤ a few writes per session). |
| Salt | **128-bit (16-byte) random per blob** | Argon2id mandate; per-blob (not per-user) since single-user-MVP. |
| Scheme tag | `"aes-gcm-256/argon2id-v1"` | The trailing `-v1` is the *cryptographic suite revision marker* — bump if we ever change parameters. session_kv already declares `aes-gcm-256/argon2id` (no -v1); the lift adds the version suffix. |

**Rejected alternatives**:

- ChaCha20-Poly1305 — equally fine cryptographically; ruled out
  only to minimize choice surface (one AEAD per release line).
- scrypt — older, equivalent threat resistance for higher param
  cost; Argon2id is the modern default.
- bcrypt — designed for password verification, not KDF for AEAD.
- PBKDF2 — acceptable but Argon2id is uniformly recommended over
  PBKDF2 in 2025 OWASP guidance for new code.

## Options considered (architecture / wire shape)

### Option A — Client-side encrypt (preserve ADR-0003 intent)

Browser uses WebCrypto subtle API. `/login` page:

1. User types `(endpoint, api_key, passphrase)`.
2. Browser derives key via WebCrypto PBKDF2 or Argon2 (WebCrypto
   has no native Argon2 — need a WASM binding like `@noble/hashes`
   or fall back to PBKDF2 client-side).
3. Browser encrypts `(endpoint, api_key)` → POSTs ciphertext +
   salt + nonce.
4. Server writes blob to session_kv; **server never sees the
   plaintext key**.
5. On dispatch: server has no plaintext API key. Either:
   (a) every dispatch requires the user to re-enter passphrase
       (UX-bad — Studio is meant for live agent streams), OR
   (b) browser holds the derived key in JS memory, dispatch round-
       trips through `/api/dispatch-with-key` that the client
       wraps with the plaintext key per call (architecturally
       awkward; SSE streams from server to browser can't easily
       carry a plaintext key from browser to provider).

**Pros**:
- Server never possesses plaintext API key. Strongest model.
- Honors ADR-0003 §"Decision" wording most literally.

**Cons**:
- (a) breaks the "live SSE dispatch" UX that M2 / M3 already ship.
- (b) requires the server to *proxy* dispatch calls through the
  browser, or for the browser to call providers directly (which
  defeats the "studio-router lives server-side with retries +
  ledger + token cost" architecture from ADR-0005 / ADR-0006).
- WebCrypto Argon2 means shipping WASM in the SvelteKit bundle
  (~50-150 KiB added to the 9 MiB binary) — measurable cost.
- No env-var fallback for headless / CLI test usage.

### Option B — Server-side derive (passphrase POSTed once, server holds in-memory key)

`/login` page:

1. User types `(endpoint, api_key, passphrase)`.
2. Browser POSTs all three over TLS / localhost to
   `POST /api/login`.
3. Server: generate per-blob salt → `argon2id(passphrase, salt)`
   → 32-byte key → AES-256-GCM-encrypt `(endpoint, api_key)`
   with that key + per-blob nonce → write `(salt || nonce ||
   ciphertext, scheme="aes-gcm-256/argon2id-v1")` to session_kv.
4. **Server holds the derived key in `axum::extract::State`-
   accessible `Arc<RwLock<Option<SessionKey>>>`** for the lifetime
   of the binary process.
5. Dispatch: pulls `EncryptedBlob` from session_kv → decrypts with
   the in-memory key → constructs `AnthropicProvider::new(...,
   api_key=plaintext)` per call → drops plaintext key from the
   stack after the provider returns the response.
6. Binary restart: in-memory key gone → first dispatch returns
   `401 NoSession` → frontend redirects to `/login` → user types
   passphrase again. (api_key + endpoint stay encrypted on disk —
   only the passphrase needs re-entry; user re-derives.)

**Pros**:
- Single round-trip on login; zero overhead per-dispatch.
- studio-router stays oblivious to crypto (gets a plain String).
- All five existing E2E specs continue to work without browser-
  layer changes — the wire shape `POST /api/login {endpoint,
  api_key, passphrase}` is what the current stub already accepts.
- Headless / CLI / test mode can bypass /login via an explicit
  `--dev-api-key <KEY>` CLI flag — clear escape hatch.
- Argon2id runs in pure-Rust server-side; no WASM in the
  SvelteKit bundle.

**Cons**:
- Server briefly possesses the plaintext API key. Mitigated by
  TLS termination (user's responsibility per threat model #2) and
  127.0.0.1-only default for single-user mode.
- Slightly weaker than Option A *in the cold-disk-theft model*?
  No — both options encrypt with a passphrase-derived key; the
  cold disk attacker sees identical ciphertext.
- Memory dump attacker (out-of-scope threat #3) reads the
  in-memory `SessionKey`. Same exposure as Option A's browser-
  side held key.

### Option C — Disk-key file outside SQLite (key on disk, not in process memory)

Generate a random 32-byte key, write to `~/.config/cobrust-
studio/key` with `0600` permissions, encrypt session_kv blobs
with it.

**Pros**: zero passphrase friction.

**Cons**: cold disk theft trivially defeats this — both the key
file and the encrypted blob live on the same machine. Provides
**no benefit over plaintext** against the in-scope threat (#1).
Rejected.

### Option D — No encryption (plaintext in session_kv)

Drop ADR-0003's "API keys never on disk in plaintext" constraint
in favor of "single-user mode, you secure your home directory."

**Pros**: trivial implementation.

**Cons**: violates ADR-0003 (explicit). Loses the
methodology-credibility-positive of "we ship the AEAD round-trip
the design called for." Rejected on design-integrity grounds —
the methodology IS the product (per ADSD case study §"Research-
product co-evolution").

## Decision

**Option B — server-side derive**. The `/login` POST carries
`(endpoint, api_key, passphrase)`; server runs Argon2id on
passphrase → 32-byte AES-256-GCM key; encrypts `(endpoint,
api_key)` JSON-serialized payload as a single ciphertext;
persists `(salt ‖ nonce ‖ ciphertext, scheme="aes-gcm-256/
argon2id-v1")` in session_kv; holds the derived key in-process
in an `Arc<tokio::sync::RwLock<Option<SessionKey>>>` for the
duration of the run.

**Storage wire format (the `EncryptedBlob` field semantics for
`scheme = "aes-gcm-256/argon2id-v1"`):**

```
ciphertext: <16-byte salt> || <12-byte nonce> || <AES-256-GCM ciphertext+tag>
nonce:      (unused — empty Vec — kept for schema compat; future
             schemes may use it again)
scheme:     "aes-gcm-256/argon2id-v1"
```

Rationale for packing salt+nonce into the `ciphertext` column:
session_kv schema already shipped in v0.1.0. Adding a `salt`
column requires a migration; packing keeps the schema stable.
The `scheme` tag is the discriminator that tells the reader how
to split the blob.

**Argon2id parameters** (the `-v1` revision marker locks these):

```
m_cost = 64 * 1024 = 65536  (64 MiB memory)
t_cost = 3                  (3 iterations)
p_cost = 1                  (1 parallel lane)
output = 32 bytes           (AES-256 key size)
```

Future parameter bumps land as `-v2`, `-v3`, etc., with the
session_kv reader matching on `scheme` to pick the right
parameter set. **Old blobs remain readable** — the scheme tag
is the version pin.

**New module**: `crates/studio-server/src/secret.rs` (~150-200
LoC target). Exports:

```rust
pub struct SessionKey([u8; 32]);
impl SessionKey {
    pub fn derive(passphrase: &str, salt: &[u8; 16]) -> Result<Self, SecretError>;
    pub fn seal(&self, payload: &EndpointSecret) -> Result<Vec<u8>, SecretError>;
    pub fn open(&self, blob: &[u8]) -> Result<EndpointSecret, SecretError>;
}

#[derive(Serialize, Deserialize)]
pub struct EndpointSecret {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("argon2id derivation: {0}")]
    Kdf(argon2::Error),
    #[error("aead seal: {0}")]
    Seal(aes_gcm::Error),
    #[error("aead open: {0}")]
    Open(aes_gcm::Error),
    #[error("malformed blob (expected {expected} bytes, got {got})")]
    Malformed { expected: usize, got: usize },
    #[error("unknown scheme: {0}")]
    UnknownScheme(String),
}
```

**API surface change** (new routes in studio-server):

| Method | Path | Behavior |
|---|---|---|
| POST | `/api/login` | Body `{endpoint, api_key, model, passphrase}` → derives + seals → writes session_kv → stores key in app state → returns `200 {status:"ok"}`. |
| POST | `/api/logout` | Drops in-memory `SessionKey` (next dispatch returns 401). session_kv blob preserved on disk. |
| GET | `/api/session/status` | Returns `{authenticated: true/false}` for frontend redirect logic. |
| GET | `/api/session/endpoint` | (debug-only, gated behind `--debug-session` flag) returns decrypted endpoint+model for E2E test introspection — **never api_key**. |

**Dispatch integration** (`router_init.rs`):

```rust
let app_state = state.clone();
let key = app_state.session_key.read().await
    .ok_or(StudioError::NotAuthenticated)?
    .clone();
let blob = state.store.session().get_endpoint().await?
    .ok_or(StudioError::NoEndpointConfigured)?;
let secret: EndpointSecret = key.open(&blob.ciphertext)?;
let provider = Arc::new(AnthropicProvider::new(
    "anthropic_official",
    &secret.endpoint,
    &secret.api_key,
)?);
// ... rest of dispatch unchanged
```

**Env-var path retention (the `--dev-api-key` escape hatch)**:

The `ANTHROPIC_API_KEY` env-var resolution path in
`studio-router::config::api_key_env` is **NOT removed**.
Instead, a new CLI flag `cobrust-studio serve --dev-api-key
<KEY> --dev-endpoint <URL>` bypasses the /login flow and
plumbs the key straight to router_init. This serves:

- Hermetic E2E tests (Playwright fixtures inject the key);
- CI integration tests that need a real provider call;
- The headless / scripted-use-case (cron jobs, CI runners);
- Developers who haven't yet visited /login post-restart.

The README post-M6 will document `/login` as the canonical
flow and `--dev-api-key` as the explicit opt-in. Sarah v2's
gate-#2 wording ("env-var workaround removed") is honored by
the **README posture change + gated CLI flag**, not by
literal code deletion.

## Done means (falsifiable success criteria)

1. **Unit tests pass** (`cargo test -p studio-server secret::`):
   - `argon2id_kdf_deterministic` — same passphrase + salt →
     same 32-byte output (verifies argon2 binding works).
   - `aes_gcm_round_trip` — encrypt-then-decrypt yields original
     plaintext.
   - `wrong_passphrase_fails_open` — different passphrase →
     `SecretError::Open` (verifies AEAD tag validation).
   - `tampered_ciphertext_fails_open` — flip one bit → fail.
   - `tampered_salt_fails_open` — flip salt → derives wrong key
     → fail.
   - `malformed_blob_too_short` — `Vec<u8>` shorter than `16 + 12
     + 16` minimum → `SecretError::Malformed`.

2. **Integration tests pass** (`crates/studio-server/tests/
   secret_roundtrip.rs`):
   - `login_then_dispatch_with_in_memory_key` — POST /api/login →
     decrypted endpoint resolves on first dispatch via
     synthetic-provider stub.
   - `restart_drops_key_returns_401` — POST /api/login → simulate
     restart (new app-state without copying RwLock) → next
     dispatch returns 401 with `NoSession` error.
   - `wrong_passphrase_login_returns_401` — POST /api/login with
     mismatched passphrase against existing session_kv blob →
     `SecretError::Open` → 401.

3. **E2E test passes** (`web/tests/e2e/login-aead.spec.ts`):
   - Playwright fixture launches binary with **no
     ANTHROPIC_API_KEY env var set** → visits /login → enters
     endpoint + key + passphrase → submits → next page (M2
     dispatch view) loads and dispatch SSE returns 200.
   - This is the **primary regression gate** — it proves the
     env-var workaround is no longer required for the happy path.

4. **Doc-coverage 7-gate stays green** with the new module:
   - `docs/agent/modules/studio-server.md` updates to enumerate
     `secret` submodule + cite ADR-0007.
   - `docs/human/zh/secret-storage.md` + `docs/human/en/secret-
     storage.md` land in same commit (zh+en parity per ADR-0001
     + M0).

5. **README update**:
   - "Known limitations" line about `ANTHROPIC_API_KEY` env-var
     workaround is removed.
   - New "Configuration" section documents `/login` as the
     primary flow + `--dev-api-key` as escape hatch.

6. **CHANGELOG entry** for v0.2.0 (M6 ships as the minor bump):
   - References ADR-0007 + the closure of Sarah v2 pilot-gate
     #2.

7. **smoke-dogfood.sh extended**:
   - New step `[5/5] POST /api/login + GET /api/session/status`
     verifies the round-trip end-to-end during release smoke.

## Phase plan (per ADSD §"Two-phase dispatch SOP")

**Phase 1 (this commit — CTO solo)**:
- This ADR landed.
- Test-skeleton commit: `crates/studio-server/tests/
  secret_roundtrip.rs` placeholder (single `#[ignore]`-attributed
  `#[test] fn placeholder()` body with `unimplemented!()`) lands
  in a follow-up commit BEFORE Phase 2 dispatch, so the P9 agent
  reads it as the test target.

**Phase 2 (P9 dispatch — 90-150 min wall-clock)**:
- Worktree: `feature/m6-aead-round-trip`.
- Deliverables: `secret.rs` module + 3 routes + dispatch
  integration + 6 unit tests + 3 integration tests + Playwright
  spec + 3 doc updates + README + CHANGELOG + smoke-dogfood.
- 7-gate green; cold rebuild from clean target/; merge `--no-ff`
  with CTO 守闸 review.

## Consequences

- **Enables**: pilot-gate #2 (Sarah v2) closure → moves
  design-partner-readiness verdict from "6 months out" to "3
  months out" if Sarah v3 confirms.
- **Enables**: Show HN post can drop the "ANTHROPIC_API_KEY env
  var workaround" caveat from the README's "Known limitations"
  list — a Mei v2 R3 confidence-blocker addressed.
- **Forecloses**: WebCrypto-only key holding (Option A) at the
  client. If we later want zero-server-trust mode, that becomes
  a flag (`--client-only-crypto`) on top of this base.
- **Forecloses**: multi-user / per-tenant key derivation in
  v0.2.x. Re-opens as Option E ("per-user salt + per-user
  in-memory key map") at v0.3.x if RBAC arrives.
- **Migration**: session_kv blobs from v0.1.x are **unencrypted
  scaffold** (the `"raw"` scheme path in
  `studio-store::session::EncryptedBlob::from<Vec<u8>>`); on
  first M6-version /login POST, the new blob overwrites the
  raw stub. No data migration required because v0.1.x users
  never had functional session_kv content.
- **Performance**: ~500 ms login wall-clock from Argon2id is
  acceptable; documented in CLAUDE.md §3.3 amendment ("crypto
  on the login hot path is intentionally slow per OWASP 2025
  KDF cost guidance").
- **Bundle size**: `aes-gcm` + `argon2` + `rand` add ~120 KiB to
  the 9 MiB binary (estimated, verified via `cargo bloat` post-
  Phase-2). Acceptable.

## Cross-references

- ADR-0001 (stack — async tokio, Rust 2024 edition)
- ADR-0003 (auth — custom-endpoint-first; this ADR closes the
  open question about server-side vs client-side crypto)
- ADR-0004 (storage — SQLite index, filesystem source of truth;
  session_kv is the only crypto-touching table)
- ADR-0006 / F-02 addendum (studio-router has zero dep on
  studio-store; this ADR preserves that)
- src: `crates/studio-store/src/session.rs:1-30` (the
  EncryptedBlob contract this ADR consumes)
- src: `crates/studio-router/src/anthropic.rs:30,42` (the
  AnthropicProvider::new(api_key) integration point)
- src: `crates/studio-router/src/config.rs:55` (api_key_env
  retention for --dev-api-key flag)
- finding (future, Phase 2): `m6-aead-round-trip-postmortem.md`
  to be filed on Phase-2 merge if any surprises surfaced
- ADSD §"Two-phase dispatch SOP" (this ADR is Phase 1)
- Sarah persona v2 pilot-gate #2 (this ADR is the response)
- Mei persona v2 R3 confidence-blocker (this ADR closes via
  README posture change in Phase 2)
