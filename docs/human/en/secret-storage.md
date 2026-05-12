# Secret Storage and AEAD Encryption (M6)

## Overview

Cobrust Studio uses **AES-256-GCM + Argon2id** encryption to protect your API key and endpoint configuration. Credentials are encrypted before being written to disk and can only be decrypted when the correct passphrase is provided.

This feature implements ADR-0007 (M6 milestone), closing Sarah persona v2's pilot-gate #2: *"AEAD round-trip ships, env-var workaround removed."*

---

## How It Works

### Login Flow

1. On the `/login` page, fill in:
   - **Endpoint URL** (e.g. `https://api.anthropic.com`)
   - **API key** (e.g. `sk-ant-...`)
   - **Model name** (e.g. `claude-opus-4-7`)
   - **Passphrase** (custom, used to encrypt the key — never stored)
2. On submit, the server:
   - Derives a 32-byte AES-256 key from the passphrase using **Argon2id** (m=64 MiB, t=3, p=1) — takes ~500 ms intentionally to resist brute force
   - Packs `(endpoint, api_key, model)` as JSON
   - Encrypts that JSON with **AES-256-GCM** using a random salt and nonce
   - Stores the ciphertext in the SQLite `session_kv` table
   - Holds the derived key in server memory for the lifetime of the process

### Storage Wire Format

```
session_kv.value = <16-byte salt> || <12-byte nonce> || <AES-GCM ciphertext+tag>
session_kv.scheme = "aes-gcm-256/argon2id-v1"
```

### Dispatch Flow

When you send a message from the `/agent` page:
1. The server reads the derived key from memory
2. Reads the encrypted blob from `session_kv`
3. Decrypts with the in-memory key to obtain the plaintext endpoint + API key
4. Passes the plaintext key to the LLM provider (discarded from the stack after the call returns)

---

## Restart Behaviour

When the `cobrust-studio serve` process restarts:
- The in-memory derived key is **cleared**
- The encrypted blob on disk is **retained**
- The next dispatch returns `401 no_session` and the frontend redirects to `/login`
- Re-entering the passphrase re-derives the key (no need to re-enter the API key)

---

## Security Properties

| Threat | Protected? | Notes |
|--------|-----------|-------|
| Cold disk theft (stolen `.cobrust-studio/db`) | ✅ Yes | Without passphrase, blob is opaque ciphertext |
| Running-process memory dump | ❌ Out of scope | Same OS-user as binary has OS-level access; single-user MVP |
| Transport-layer interception | ❌ Out of scope | TLS termination is the operator's responsibility |
| Multi-user / per-tenant key isolation | ❌ Out of scope | Deferred to v0.3.x if RBAC arrives |

---

## Developer Escape Hatch (`--dev-api-key`)

For CI, Playwright fixtures, and headless scripts, you can bypass the `/login` flow:

```bash
cobrust-studio serve \
  --project /path/to/project \
  --dev-api-key sk-ant-xxx \
  --dev-endpoint https://api.anthropic.com \
  --dev-model claude-opus-4-7
```

The server boots in an already-authenticated state. **`/login` remains the canonical primary flow for interactive use**; `--dev-api-key` is an explicit opt-in.

Environment variable equivalents are also supported:

```bash
export COBRUST_DEV_API_KEY=sk-ant-xxx
export COBRUST_DEV_ENDPOINT=https://api.anthropic.com
export COBRUST_DEV_MODEL=claude-opus-4-7
cobrust-studio serve --project /path/to/project
```

---

## Performance — Argon2id wall-clock

Argon2id is intentionally slow. The interactive-login latency is set by
the `m_cost / t_cost / p_cost` parameters in
`crates/studio-server/src/secret.rs::SessionKey::derive`. Current values
(`-v1` scheme): `m=64 MiB, t=3, p=1, out=32 B`.

Measured (release-mode build):

| Hardware | Median wall-clock (N=5) |
|---|---|
| Apple M4 (2024 MacBook) | **70 ms** |
| Apple M2 (estimated) | ~120 ms |
| GitHub Actions ubuntu-latest runner (2 vCPU shared) | ~300-400 ms estimated |
| Old laptop (2018-era Intel i5) | ~500-800 ms estimated |

Hard ceiling is 2 seconds, enforced by `secret::tests::bench_argon2id_derive`
in release mode. If your hardware exceeds that, file a finding —
`m_cost` may need tuning down for that target class. Run the bench:

```bash
cargo test --release -p studio-server --lib -- --ignored --nocapture bench_argon2id_derive
```

Future parameter revisions bump the scheme tag to `-v2`, `-v3`, etc.
Old blobs remain readable because the scheme tag is the version pin
(see ADR-0007 §"Storage wire format").

---

## Rotating your passphrase

There is **no `POST /api/change-passphrase` route in v0.2.x** — that's
slated as an ADR-pending v0.3.x enhancement. Until then, the procedure
is:

1. Stop the server.
2. Delete the session_kv row that holds the encrypted blob:
   ```bash
   sqlite3 .cobrust-studio/studio.db "DELETE FROM session_kv WHERE key = 'endpoint';"
   ```
3. Start the server.
4. Visit `/login` and submit the new passphrase + your endpoint / API
   key / model. Studio seals a fresh blob.

This forgets the old encrypted blob entirely. There's no
"verify-old-then-rotate" flow yet — the deletion approach is the only
path that doesn't require knowing the old passphrase, which matters
for the "I forgot my passphrase" case.

---

## Related Documents

- ADR-0007: Secret storage AEAD round-trip design decision
- ADR-0008: Multi-provider /login (v0.3.x, Phase 2 pending)
- ADR-0003: Auth model (custom-endpoint-first)
- `crates/studio-server/src/secret.rs`: Implementation
- `crates/studio-server/src/routes/login.rs`: Route handlers
