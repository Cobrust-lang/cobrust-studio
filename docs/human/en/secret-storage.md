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

---

## Provider selection (M7, ADR-0008)

Starting in v0.3.0 the `/login` page shows a **Provider** dropdown between
the Model field and the Passphrase field.

### Dropdown options

| Value | Label | Use for |
|-------|-------|---------|
| `anthropic` | Anthropic API | `api.anthropic.com` or compatible shims |
| `openai` | OpenAI-compatible (vLLM / DeepSeek / Together / OpenRouter / Groq / Ollama) | Any `POST /chat/completions` endpoint |

### URL hint

As you type the Base URL, the form auto-suggests a provider kind:

- URL contains `anthropic.com` → dropdown auto-selects **Anthropic API**.
- URL is non-empty and does not contain `anthropic.com` → dropdown
  auto-selects **OpenAI-compatible**.

You can override the suggestion by changing the dropdown manually.

### Wire format

`provider_kind` is an additive field in the POST body:

```json
{
  "endpoint": "https://api.openai.com/v1",
  "api_key": "sk-...",
  "model": "gpt-5",
  "passphrase": "...",
  "provider_kind": "openai"
}
```

Omitting `provider_kind` (e.g. old curl scripts) defaults to `"anthropic"`,
preserving backward compatibility with v0.2.x.

### Synthetic provider rejected at /login

`provider_kind: "synthetic"` is rejected by the server with
`400 { code: "invalid_provider_kind" }`. The synthetic provider is a
CLI/dev-only construct (see `--dev-api-key` below) with no real-world
endpoint + key pair; submitting it via the login form is a category error.

### `--dev-api-key` + `--dev-provider-kind`

The `--dev-api-key` CLI flag (and `COBRUST_DEV_API_KEY` env var) can be
combined with `--dev-provider-kind` to inject an OpenAI-compat session at
boot without going through `/login`:

```bash
cobrust-studio serve \
  --project /path/to/project \
  --dev-api-key sk-... \
  --dev-endpoint https://api.openai.com/v1 \
  --dev-model gpt-5 \
  --dev-provider-kind openai
```

Defaults to `anthropic` if `--dev-provider-kind` is omitted (v0.2.x
backward compat).

---

## Persistent session backends (M8, ADR-0009)

By default, the in-memory `SessionKey` is **dropped on every binary
restart** — you re-enter your passphrase via `/login` to re-derive it.
For a dev-laptop workflow this is fine (~70 ms re-derive on Apple M4).

For **long-lived deployments** (systemd unit, Docker container,
headless server reboot) this is friction that compounds. Starting in
v0.4.0, `cobrust-studio serve` accepts an opt-in `--persist-session`
flag that wraps your passphrase in one of three backends. The next
boot auto-unlocks the session — no `/login` round-trip needed.

### Three modes

| Mode | CLI | At-rest store | Trust model |
|------|-----|---------------|-------------|
| `none` (default) | `--persist-session=none` | nothing — `SessionKey` dies with the process | v0.3.0 baseline; re-enter passphrase per restart |
| `keychain` | `--persist-session=keychain` | OS keychain (macOS Keychain / freedesktop secret-service / Windows Credential Manager via DPAPI) | Strongest cold-disk-theft posture; the passphrase lives in the user-scoped keychain, never on disk |
| `file` | `--persist-session=file --persist-session-file=/path/to/passphrase` | `0600` mode plaintext file | Sysadmin-friendly fallback for environments without a keychain (Docker, headless Linux without D-Bus); same trust model as `--dev-api-key` (operator-bounded) |

Default is `none` — opt-in only. Existing v0.3.x deployments see
**zero behavior change** until they pass the flag.

### Quick start — keychain backend (dev laptops, single-user servers)

```bash
cobrust-studio serve \
  --project /path/to/project \
  --persist-session=keychain
```

On first login, your passphrase is written to the OS keychain under
service `cobrust-studio`, username slot `session-passphrase`. On the
next restart, the server reads it back, re-derives the session key,
and you're authenticated without visiting `/login`.

To clear (e.g. handing the laptop back, rotating credentials):

```bash
# macOS:
security delete-generic-password -s cobrust-studio -a session-passphrase
# Linux (gnome-keyring / KWallet):
secret-tool clear service cobrust-studio username session-passphrase
# Windows (PowerShell):
cmdkey /delete:cobrust-studio
# Or via the API:
curl -X POST "http://localhost:7878/api/logout?purge=true"
```

### Quick start — file backend (Docker, systemd, headless without D-Bus)

```bash
cobrust-studio serve \
  --project /path/to/project \
  --persist-session=file \
  --persist-session-file=/etc/cobrust-studio/passphrase
```

The file is created on first `/login` with mode `0600` (Unix only —
Windows skips this check; prefer keychain on Windows). Subsequent
boots read it; the server auto-unlocks.

To clear:

```bash
rm /etc/cobrust-studio/passphrase
# Or via the API:
curl -X POST "http://localhost:7878/api/logout?purge=true"
```

Environment-variable equivalents:

```bash
export COBRUST_PERSIST_SESSION=file
export COBRUST_PERSIST_SESSION_FILE=/etc/cobrust-studio/passphrase
cobrust-studio serve --project /path/to/project
```

### Security trade-off table

| Threat | `none` | `keychain` | `file` |
|--------|--------|------------|--------|
| Cold disk theft (stolen `.cobrust-studio/db`) | Protected (passphrase needed) | Protected (passphrase in keychain is OS-user-scoped, not on disk image) | **Weakened** (file is on disk; attacker with disk + file = full unlock) |
| Sysadmin / OS-user-equivalent attacker (same user as the server) | Out of scope (= ADR-0007 §"Threat model" #3) | Out of scope (same trust level wins keychain access) | Out of scope (same trust level can read the 0600 file) |
| Container escape | Depends on deployment | Strongest — keychain is host-bound | Worst — file is in the container fs |
| Docker container restart with bind-mounted persist file | N/A | N/A (host keychain typically not visible) | **Works** — survival is the entire point of this mode |
| Operator forgets passphrase, no key in keychain/file | Re-login normally | Re-login normally (keychain doesn't recover a forgotten one) | Re-login normally |

**The fundamental property**: disk theft alone is still defeated by the
keychain backend (the passphrase doesn't appear in the disk image).
The file backend exists for deployments where a keychain is
unavailable — choose `keychain` if your environment supports it;
fall back to `file` for Docker / D-Bus-less Linux / NixOS modules /
Kubernetes operators.

### What survives a binary restart with `--persist-session=keychain|file`?

```
[--persist-session=keychain or =file]
  /login → seal blob + store key in memory + MIRROR passphrase to backend
  [restart]
  boot → load passphrase from backend → derive(blob[..16] salt) → VERIFY open(blob) → set in-memory key
  /api/session/status → authenticated=true (no /login round-trip)
```

**Verification step** (the M6 seal-salt-mismatch lesson): the boot
flow doesn't just trust the persist entry. It re-derives the key from
the persist passphrase, then calls `key.open(&blob.ciphertext)` to
prove the derived key matches the blob. If the open fails (passphrase
rotated externally without clearing persist; blob corrupted), the
persist entry is **auto-cleared** and you fall back to `/login`. This
prevents the "I rotated my passphrase via sqlite3 but forgot to purge
the keychain" hazard from masquerading as a successful auto-unlock.

### `/api/logout?purge=true`

A normal `POST /api/logout` drops the in-memory key (so the next
`/api/dispatch` returns 401) but **preserves** the persist backend —
you can re-login by simply restarting the server (the backend
auto-unlock fires again).

`POST /api/logout?purge=true` ALSO clears the persist backend
(keychain entry / file). Use this when you want a fresh `/login`
flow on the next boot — e.g. handing the laptop back, rotating
credentials, demoing the product without your real session bleeding
through.

### Long-lived deployments (systemd, Docker)

The README §"Configuration" section has the recommended deployment
recipes. The short version:

- **systemd**: `--persist-session=keychain` if the unit runs under a
  user with a D-Bus session (`linger` enabled on Linux). Otherwise
  use `--persist-session=file` with a path under `/etc/cobrust-
  studio/` and ensure the unit's `User=` directive owns that path.
- **Docker**: prefer `--persist-session=file` with the passphrase
  file bind-mounted into the container. Host the file outside the
  image so `docker build` cache can't accidentally bake the
  passphrase into a layer.

---

## Related Documents

- ADR-0007: Secret storage AEAD round-trip design decision
- ADR-0008: Multi-provider /login (v0.3.0, Phase 2 implemented)
- ADR-0009: Persistent session across binary restart (v0.4.0, M8)
- ADR-0003: Auth model (custom-endpoint-first)
- `crates/studio-server/src/secret.rs`: AEAD implementation
- `crates/studio-server/src/persist.rs`: M8 persist backends
- `crates/studio-server/src/routes/login.rs`: Route handlers
