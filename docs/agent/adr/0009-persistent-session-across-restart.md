---
adr_id: "0009"
title: M8 persistent session across binary restart — OS keychain wrap + passphrase-file fallback
status: proposed
date: 2026-05-12
supersedes: []
superseded_by: []
---

# ADR-0009: persistent session across binary restart (M8)

## Context

v0.2.0 shipped M6 AEAD round-trip (ADR-0007); v0.3.0 shipped M7
multi-provider /login (ADR-0008). Current behavior:

1. User visits `/login`, submits `(endpoint, api_key, model, passphrase,
   provider_kind)`.
2. Server derives Argon2id key, seals the `EndpointSecret`, stashes
   the `SessionKey` in `AppState`.
3. On binary restart, **`AppState.session_key` is `None`** → first
   dispatch returns 401 NoSession → user must re-enter passphrase via
   `/login`.

For interactive dev-laptop usage this is fine (~70 ms re-derive on
M4 per the v0.3.0 bench). For **long-lived deployments** — systemd
unit, Docker container, headless server reboot — this is friction
that compounds:

- A crash + auto-restart loop wedges in 401 limbo until a human
  notices and re-enters the passphrase.
- A Docker container restart for a deploy or host reboot loses
  the session.
- A sysadmin rotating from one machine to another can't migrate the
  running session.

Sarah v3 audit (post-M6) flagged this as a v0.3.x gate:

> "Persistent session across binary restart (passphrase-cached, not
> stored). Currently every restart requires re-entering the
> passphrase... For a design-partner team running Studio as a long-
> lived server process (systemd unit, Docker container), this is
> friction at best, adoption-blocking at worst."

Sarah v4 confirmed: "Someone on the team is willing to re-enter
their passphrase on binary restart. For a dev-laptop workflow this
is acceptable; for a server deployment it's annoying."

M8 closes the gap. The session_key must survive binary restart on
servers that opt in.

## Hard constraints from prior ADRs

- **ADR-0007 §"Architecture pin"**: server-side derive, in-memory
  SessionKey, server holds plaintext briefly. M8 inherits.
- **ADR-0007 §"Storage wire format"**: `salt(16) || nonce(12) ||
  ciphertext+tag` under scheme `"aes-gcm-256/argon2id-v1"`. M8 does
  NOT touch the session_kv blob format.
- **ADR-0007 §"Threat model"**: cold disk theft is in-scope (must
  remain protected by passphrase); running-process memory dump is
  out-of-scope (single-user, OS-user access defeats any in-process
  crypto).
- **ADR-0008 §"Threat model additions"**: provider-kind tampering
  caught by GCM tag. M8 inherits.

## Threat model (M8 additions)

The persistent-session feature trades **passphrase re-entry friction**
for some **at-rest security degradation**, scoped to the wrap layer:

1. **Disk-only cold attacker (in-scope today)**: needs the
   passphrase. M8 preserves this for the **OS keychain** path
   (keychain entry is OS-user-scoped). The **0600 plaintext file**
   fallback weakens this posture: an offline attacker who obtains both
   the passphrase file and `session_kv` blob can unlock the stored
   endpoint secret.
2. **Disk + OS-user-access attacker**: this attacker can read both
   the keychain (it's their user) and the session_kv blob, OR can
   read both the 0600 file and the blob. **M8 cannot defend against
   this attacker** — they have the same trust level as the running
   server. Out of scope, same as the original ADR-0007 #3 (memory
   dump).
3. **Container escape attacker** (NEW under M8): in a Docker
   deployment the keychain layer is the container's emulated KMS or
   the host's keychain via volume mount. The threat depends on the
   host's container configuration; document the trade-offs but
   don't prescribe (each user's deployment is different).

The fundamental security property: **disk theft alone is still
defeated only by the keychain backend**. The file backend is an
operator-chosen fallback for deployments without a usable keychain;
it trades cold-disk-theft posture for restart survival.

## Options considered

### Option A — OS keychain wrap (via the `keyring` crate)

Use [the `keyring` crate](https://crates.io/crates/keyring) for
cross-platform OS keychain access:

- **macOS**: stores the passphrase in the user's login Keychain via
  the Security framework. Access is scoped to the calling
  application's bundle id + user.
- **Linux**: uses freedesktop's `org.freedesktop.secrets` D-Bus
  interface (gnome-keyring / KWallet / secret-service-tool). Access
  is scoped to the D-Bus session.
- **Windows**: uses DPAPI's Credential Manager. Access is scoped to
  the calling user.

Wire flow:
1. User logs in via `/login` with passphrase P.
2. Server derives `key = SessionKey::derive(P, salt)` as before.
3. **NEW**: server stores `P` in the OS keychain under the entry
   `"cobrust-studio-passphrase"` (or a project-rooted equivalent
   for multi-instance support).
4. On restart, server reads `P` from the keychain → re-derives `key`
   from `P + salt-extracted-from-session-kv-blob[..16]` → stashes
   key in `AppState.session_key` automatically. No `/login` prompt.

**Pros**:
- Strongest cold-disk-theft posture: the passphrase never lands on
  disk in any form.
- Stable, audited cross-platform Rust crate (~10K stars,
  RustCrypto-adjacent maintainers).
- Zero workflow change for the user once enabled — no extra files
  to manage.

**Cons**:
- **macOS Daemon mode**: a launchd-managed cobrust-studio daemon
  may not have access to the user's login Keychain (the keychain
  is gated by user session). Documented mitigation: store in the
  system keychain (requires admin to grant access at install time),
  or run cobrust-studio under launchctl-asuser.
- **Linux without D-Bus session**: systemd-unmanaged daemons + raw
  container environments often have no D-Bus session bus. The
  `keyring` crate returns an error; M8 falls through to Option B
  (passphrase file) per the §"Fallback chain" below.
- **Docker without keychain access**: containers don't have host
  keychain access by default. Same fallback applies.
- ~1.2 MB binary size add for the `keyring` crate + dependencies.

### Option B — Plaintext passphrase file (0600, fs::permissions)

Store the passphrase in a file under `~/.config/cobrust-studio/
passphrase` (or `$COBRUST_STUDIO_PASSPHRASE_FILE` if set) with
permissions `0600` (owner read/write only). On Linux + macOS this
restricts to the running user; on Windows we'd need ACL via
`windows-acl` crate (or just document the gap).

The file content is the **plaintext passphrase**, not the derived
key — because the salt lives in the session_kv blob and we need
the passphrase to re-derive on restart.

**Pros**:
- Works in every environment (no D-Bus, no Keychain, no Credential
  Manager required).
- Trivial to implement (~30 LOC).
- Sysadmin-friendly — backup / restore is just `cp passphrase`.

**Cons**:
- Disk theft + file read = full unlock. The wrap secret IS on disk,
  protected only by file permissions. An attacker with root access
  defeats this trivially.
- This is the same trust model as `--dev-api-key`'s
  `COBRUST_DEV_API_KEY` env var — equally weak against disk theft.

### Option C — TPM / HSM-backed wrap (out of scope)

Use TPM 2.0 (Linux/Windows) or Secure Enclave (macOS) to wrap the
key. Strongest model but introduces hardware dependency, vendor
lock-in (Secure Enclave only on Apple Silicon), and complex
fallback logic. Out of scope for v0.3.x.

### Option D — Encrypted-at-rest passphrase file using a derived OS-user key

Use a per-OS-user derived key (e.g., from the user's home directory
inode + system entropy) to AES-GCM-encrypt the passphrase file.
This sits between A and B — file-based but with weak OS-scoped
crypto. The derived key is reproducible at restart by the same
OS user.

**Pros**: works everywhere; better than plaintext file.

**Cons**: the "derived" key isn't really a secret — anyone with
disk access can re-derive it. This is essentially obfuscation, not
encryption. **Rejected**.

### Option E — Don't ship the feature; document the workaround

Tell users to use `--dev-api-key` for headless servers. This is
the v0.2.x/v0.3.x status quo. Sarah v4 verdict says it's annoying
but not blocking for the 1-5 person team. **Rejected** because
the larger-team adoption gate Sarah v4 named requires it.

## Decision

**Option A (OS keychain via the `keyring` crate) as primary +
Option B (0600 passphrase file) as documented fallback.**

The two paths are explicitly opt-in via CLI flags, off by default:

```
cobrust-studio serve \
  --project /path \
  --persist-session=keychain        # Option A — primary
# or
cobrust-studio serve \
  --project /path \
  --persist-session=file            # Option B — fallback
  --persist-session-file=/etc/cobrust-studio/passphrase.txt
```

Default: `--persist-session=none` (current v0.3.0 behavior; user
re-enters passphrase on every restart).

### Wire detail — keychain path

```rust
fn save_session_to_keychain(passphrase: &str) -> Result<(), KeychainError> {
    let entry = keyring::Entry::new("cobrust-studio", "session-passphrase")?;
    entry.set_password(passphrase)
}

fn load_session_from_keychain() -> Result<String, KeychainError> {
    let entry = keyring::Entry::new("cobrust-studio", "session-passphrase")?;
    entry.get_password()
}
```

The keyring entry's service name is `"cobrust-studio"`; the username
slot is `"session-passphrase"`. Both are constant — single-instance
assumption holds for v0.3.x (consistent with CLAUDE.md §1).

### Wire detail — file path

```rust
fn save_session_to_file(passphrase: &str, path: &Path) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(passphrase.as_bytes())
}

fn load_session_from_file(path: &Path) -> io::Result<String> {
    // Verify permissions are 0600 (fail-open if relaxed).
    let meta = fs::metadata(path)?;
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(io::Error::new(
            ErrorKind::PermissionDenied,
            format!("passphrase file has mode {mode:o}; must be 0600"),
        ));
    }
    fs::read_to_string(path)
}
```

Windows handling: skip the mode check (Windows permission model
differs); document the gap.

### Boot flow with persistent session

```
on_serve_start:
    match cli.persist_session:
        Mode::None => app_state.session_key = None   # v0.3.0 behavior
        Mode::Keychain =>
            try load_session_from_keychain():
                Some(passphrase) =>
                    blob = store.session().get_endpoint().await?
                    salt = blob.ciphertext[..16]
                    key = SessionKey::derive(passphrase, salt)
                    app_state.session_key = Some(key)
                None or Err =>
                    log warn ("keychain access failed; falling back to /login")
                    app_state.session_key = None
        Mode::File(path) =>
            similar but reading the file with permission check
```

### On `/api/login` success in persist-mode

When `--persist-session=keychain|file` is enabled AND the login
succeeds, the route handler **additionally** persists the passphrase
to the chosen backend. The persistence is a side effect of login,
not a separate route.

This means: if the user changes their passphrase via the rotation
procedure (delete blob → re-login), the keychain/file entry is
overwritten with the new passphrase. No stale-entry drift.

### On `/api/logout`

Logout MUST clear the in-memory key. By default, logout does NOT
clear the keychain/file entry (the user can still re-login by
restart without typing the passphrase). For "I want to fully
forget this credential" semantics, a new route `POST
/api/logout?purge=true` clears the keychain/file entry too. This
is the equivalent of a passphrase rotation followed by logout.

## Done means (falsifiable success criteria)

1. **Unit tests** (`cargo test -p studio-server keychain_wrap::`):
   - `keychain_round_trip` — save + load returns the same passphrase
     (gated by `#[cfg(target_os = ...)]` since CI environments may
     lack a keychain).
   - `file_round_trip` — save with 0600 + load + verify the
     permission check.
   - `file_load_rejects_loose_permissions` — write 0644 file →
     load returns PermissionDenied.

2. **Integration tests** (`crates/studio-server/tests/
   persistent_session.rs`, NEW):
   - `keychain_path_survives_restart` — POST /api/login with
     `--persist-session=keychain` → simulate restart (new AppState
     instance + keychain re-read) → status authenticated=true
     without re-login.
   - `file_path_survives_restart` — same with file backend.
   - `none_path_drops_key_on_restart` — same with default mode →
     status authenticated=false after restart.
   - `logout_purge_clears_keychain` — POST /api/logout?purge=true
     → simulate restart → no auto-unlock.

3. **E2E test** (NOT shipped in M8; documented as out-of-scope
   because Playwright can't observe keychain state).

4. **Workspace deps**: `keyring = "3"` added (under the `[target]`
   `cfg-not(target_arch = "wasm32")` group, since WASM doesn't ship
   the binary anyway).

5. **CLI flag tests** in `crates/studio-server/tests/cli_args.rs`
   (NEW or extend): `--persist-session=keychain` / `=file` /
   `=none` parse correctly; `--persist-session-file <path>` is
   required when mode=file.

6. **Docs**:
   - `docs/agent/modules/studio-server.md` — new §"Persistent
     session backends" enumerating keychain / file / none.
   - `docs/human/{zh,en}/secret-storage.md` — Persistence section
     describing each backend + trade-offs.
   - `README.md` — "Configuration" §gains a "Persistent session
     (servers)" subsection describing both backends + Docker /
     systemd recommendation.

7. **CHANGELOG entry** for v0.4.0:
   - References ADR-0009 + closes Sarah v3/v4 Gate B.

## Phase plan (per ADSD §"Two-phase dispatch SOP")

**Phase 1 (this commit, CTO solo)**:
- This ADR landed.
- Optional: test-skeleton placeholder for `tests/
  persistent_session.rs`.

**Phase 2 (P9 dispatch, ~120-180 min wall-clock — bigger than M6/M7
because cross-platform CI matrix work is involved)**:
- Worktree: `feature/m8-persistent-session`.
- Deliverables: `keychain_wrap` module + 3 CLI flags + boot-flow
  integration + 3 unit + 4 integration tests + zh/en/module docs
  + README + CHANGELOG + `keyring` workspace dep.
- 7-gate green; CI on 3 OS matrix; CTO 守闸; merge --no-ff.

**Phase 2 caveats**:
- macOS Keychain access from a non-bundled binary in CI may prompt
  a GUI dialog. The integration tests on `macos-latest` may need to
  use a temporary keychain (via `security create-keychain` + `set
  -d -k` workaround) to avoid CI hang.
- Linux secret-service requires a D-Bus session. CI on `ubuntu-
  latest` may need to spin up `dbus-launch` + `gnome-keyring-
  daemon --unlock` before the test. P9 implementer documents the
  CI fixture pattern.
- Windows Credential Manager generally works in CI but the
  `keyring` crate's Windows backend requires explicit user context
  (which CI runners have via `runneradmin`).
- A CI-only fallback to a no-op keychain backend may be needed for
  tests that don't actually exercise the platform-specific
  storage; mark those tests with `#[ignore]` if they fail to
  acquire a keychain handle.

## Consequences

- **Enables**: Sarah v3/v4 Gate B closure (persistent session
  across restart). Adoption gate for systemd/Docker/long-lived-
  server use cases.
- **Enables**: ADSD methodology demonstration of **opt-in
  security/friction trade-offs** as first-class API surface (the
  3-mode flag is the user-visible knob).
- **Forecloses**: TPM / Secure Enclave integration (Option C) for
  v0.4.x. Reopens if a design partner explicitly requires it.
- **Migration**: zero — opt-in flag, default behavior unchanged
  from v0.3.0. Pre-M8 deployments that haven't set the flag see
  identical UX. Users opting in must explicitly enable the backend
  via `--persist-session=`.
- **Workspace size**: `keyring` adds ~1.2 MB to the 9 MB binary.
  Acceptable. Document in `cargo bloat` survey post-Phase-2.
- **CI matrix**: existing 3-platform test matrix gains a new
  `persistent_session` test target. macOS / Linux / Windows each
  exercise their backend. Adds ~30-60 s to each platform's test
  job (~3-5 min total CI time across all platforms).
- **Performance**: keychain reads are sub-millisecond on warm OS
  cache; cold reads (first boot after OS restart) may take 5-50ms.
  Argon2id re-derive (~70ms M4 / ~300-400ms ubuntu CI) dominates.
  No optimization needed.

## Cross-references

- ADR-0001 (stack — async tokio, Rust 2024 edition)
- ADR-0003 (auth — custom-endpoint-first; M8 keeps the form-driven
  posture; persistence is opt-in additive layer)
- ADR-0007 (M6 AEAD round-trip — M8 extends the boot flow without
  changing the session_kv wire format)
- ADR-0008 (M7 multi-provider /login — M8 inherits; persist mode
  saves the passphrase + provider_kind survives via the existing
  session_kv blob path)
- src: `crates/studio-server/src/secret.rs::SessionKey::derive`
  (the function M8 will call after extracting passphrase from
  keychain or file)
- src: `crates/studio-server/src/state.rs::AppState::session_key`
  (the slot M8 boot flow will populate)
- src: `crates/studio-server/src/cli.rs::ServeArgs` (extend with
  `--persist-session` + `--persist-session-file`)
- src: `crates/studio-server/src/router_init.rs::init_router` (or
  a new `crates/studio-server/src/auth_resume.rs`) — the boot-time
  unlock hook
- Sarah v3 audit finding (B): "Persistent session across restart"
- Sarah v4 audit gate B refinement
- `keyring` crate: https://crates.io/crates/keyring
