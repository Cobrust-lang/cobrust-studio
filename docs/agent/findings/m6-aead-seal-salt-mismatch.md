---
doc_kind: finding
finding_id: m6-aead-seal-salt-mismatch
last_verified_commit: bc9e624
discovered_by: Playwright hermetic e2e (login-aead.spec.ts test 2 + login.spec.ts) running against the v0.2.0 release binary; surfaced via the natural re-derive path that the unit-test corpus did not exercise
severity: P0
status: closed_by_v0.2.1
dependencies: [adr:0007]
related: [m4-release-readiness-spa-fallback-extractor, cto-shougate-test-gate-grep-leak, f20-closure-last-verified-commit-enforcement]
---

# Finding: `SessionKey::seal()` packed a fresh random salt instead of the derive salt, silently breaking the AEAD re-derive round-trip

## Hypothesis

The M6 P9 dispatch (sonnet 4.6, ADR-0007 Phase 2) shipped a
`SessionKey::seal()` implementation that **generated a new random
salt on every call** and packed it into the blob header alongside
the nonce and ciphertext. The hypothesis: this preserved freshness
of the wire-format header but didn't break anything because the
unit-test corpus all round-tripped through the same `SessionKey`
instance (`key.seal(); key.open()`).

The hypothesis was wrong. The blob's packed salt is the **input**
to the re-derive path in `crates/studio-server/src/routes/login.rs`
when an existing blob is present: `derive(req.passphrase,
blob[..16])` is supposed to reproduce the key that originally
sealed the blob. With a freshly-randomized seal salt, the packed
salt did not match the derive salt → re-derive produced a different
key → AEAD tag check failed → the route returned
`400 wrong_passphrase` for any login that touched an existing
session_kv blob with the **correct** passphrase.

## Failure mode

Concrete reproduction (against the v0.2.0 release binary):

```bash
$ ./cobrust-studio serve --project /tmp/repro --port 27890 &
# t=0: first login — succeeds, blob written
$ curl -X POST http://127.0.0.1:27890/api/login \
    -H 'content-type: application/json' \
    -d '{"endpoint":"https://api.anthropic.com","api_key":"sk-A",
         "model":"claude-opus-4-7","passphrase":"playwright-test-passphrase-m6"}'
{"status":"ok"}
$ curl http://127.0.0.1:27890/api/session/status
{"authenticated":true}
# Logout — drops in-memory key, leaves blob on disk
$ curl -X POST http://127.0.0.1:27890/api/logout
{"status":"ok"}
# t=1: second login — SAME passphrase, different api_key
$ curl -X POST http://127.0.0.1:27890/api/login \
    -H 'content-type: application/json' \
    -d '{"endpoint":"https://api.anthropic.com","api_key":"sk-B",
         "model":"claude-opus-4-7","passphrase":"playwright-test-passphrase-m6"}'
{"error":"passphrase does not match existing credential blob","code":"wrong_passphrase"}
# ← FALSE POSITIVE. The passphrase is correct.
```

The user's experience: "I just logged in successfully a moment ago
with passphrase P; now I'm being told P is wrong." Effectively a
total denial of the M6 round-trip beyond first login.

## Root cause

`crates/studio-server/src/secret.rs::seal()` at the time of the bug:

```rust
pub fn seal(&self, payload: &EndpointSecret) -> Result<Vec<u8>, SecretError> {
    let json = serde_json::to_vec(payload)?;
    let mut salt = [0u8; SALT_LEN];  // ← fresh random salt
    OsRng.fill_bytes(&mut salt);     // ← packed into blob header
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let key = Key::<Aes256Gcm>::from_slice(&self.0);  // ← but encrypted with self
    // ... encrypt with self.0 and pack salt||nonce||ciphertext ...
}
```

`SessionKey` was a tuple struct `pub struct SessionKey([u8; 32])`
holding only the 32-byte AES key. It did **not** carry the salt
that the key was derived from. So when `seal()` ran:

- The 32-byte key in `self.0` was derived from `passphrase + salt_A`
  at login time (`SessionKey::derive(passphrase, &salt_A)`).
- `seal()` randomly generated `salt_B ≠ salt_A` and packed it into
  the blob header.
- Re-derive on next login: `derive(passphrase, salt_B) → key_B ≠ key_A`.
- `key_B.open(blob)` fails because the blob was encrypted under `key_A`
  → AEAD tag mismatch → `SecretError::Open`.

The wire-format contract in ADR-0007 §"Storage wire format" was
**implicit** about whether the salt was the derive salt or
seal-time-random. The decision is intentional for the unit-test
corpus (P9 wrote 6 unit tests that all match the contract
"salt is whatever seal generates"), but **wrong** for the
re-derive path that login.rs takes.

## Forward implications

### Test corpus had a structural blind spot

All 6 M6 P9 unit tests round-tripped through the **same** `SessionKey`:

```rust
let key = SessionKey::derive(passphrase, &salt).unwrap();
let blob = key.seal(&secret).unwrap();
let recovered = key.open(&blob).unwrap();  // ← same key both sides
```

The re-derive path was structurally invisible to this corpus
because no test ever did:

```rust
let key1 = SessionKey::derive(passphrase, &salt).unwrap();
let blob = key1.seal(&secret).unwrap();
// Simulate restart / new-login arrival:
let salt_from_blob = &blob[..16];
let key2 = SessionKey::derive(passphrase, salt_from_blob).unwrap();
let recovered = key2.open(&blob).unwrap();  // ← would have failed pre-fix
```

This is **textbook ADSD F1.0**: "declared-invariant gap" — the
invariant ("packed salt enables re-derive") was stated in
ADR-0007 §"Wire format" but the test corpus did not exercise the
path that would prove it. A persona-driven E2E test
(`login-aead.spec.ts` test 2 + `login.spec.ts`) caught the bug
because it naturally exercised the re-derive path. The unit-test
corpus alone would never have caught this.

### F1.5 sub-form proposed (ADSD catalogue v1.2.7)

This bug is the empirical anchor for the new F1.5 entry: **"test-
corpus structural blind spot on re-derive paths"**. Future cycles
should ask, for any `seal/open`-shaped API: *does the test corpus
exercise the case where seal and open are performed by DIFFERENT
instances of the same logical key*? If not, file a missing test.

## Recovery

Fixed at commit `3753a2b`:

1. `SessionKey` is now a named-field struct carrying the salt:

   ```rust
   #[derive(Clone)]
   pub struct SessionKey {
       key: [u8; 32],
       salt: [u8; 16],
   }
   ```

2. `derive(passphrase, &salt)` stores `*salt` in the returned key.

3. `seal()` packs `self.salt` (not a fresh random salt) into the
   blob header. Nonce remains fresh per seal (AES-GCM nonce-
   uniqueness requirement).

4. New unit test `seal_then_re_derive_then_open_round_trips`
   locks the contract:

   ```rust
   let key1 = SessionKey::derive(passphrase, &salt).unwrap();
   let blob = key1.seal(&secret).unwrap();
   assert_eq!(&blob[..16], &salt, "packed salt must equal derive salt");
   let mut blob_salt = [0u8; 16];
   blob_salt.copy_from_slice(&blob[..16]);
   let key2 = SessionKey::derive(passphrase, &blob_salt).unwrap();
   assert_eq!(key1.key, key2.key);
   let recovered = key2.open(&blob).unwrap();
   ```

This test would have failed pre-fix; it passes post-fix. The
catalogue invariant — "blobs sealed by an M6 binary must be
openable by a future M6 binary that re-derives from the packed
salt + the same passphrase" — now has script-level enforcement.

## Prevention

### Authored guidance for future seal/open APIs

For any newly-authored `derive` + `seal` + `open` API in this
project (or any project applying ADSD discipline):

1. **Salt belongs to the key, not the seal call**. The KDF
   produces a key whose only valid encrypter is a holder of the
   `(passphrase, salt)` pair that produced it. Seal MUST pack
   `self.salt`, not a fresh randomization. Random per-seal goes
   in the nonce, where AES-GCM mandates it.

2. **Test corpus must include a "different-instance re-open"
   test**. Pattern:

   ```rust
   let k1 = derive(P, S);
   let blob = k1.seal(payload);
   let salt_from_blob = blob[..SALT_LEN];
   let k2 = derive(P, salt_from_blob);
   assert!(k1.bytes() == k2.bytes());
   let recovered = k2.open(blob);
   assert_eq!(recovered, payload);
   ```

3. **The ADR's "wire format" section is binding documentation,
   not aspirational documentation**. If the ADR says "packed salt
   enables re-derive," the test corpus is responsible for proving
   that. If a P9 sub-agent writes the implementation, the CTO
   守闸 should re-read the ADR + the test corpus together and ask
   "does the corpus exercise the wire-format invariants?"

### ADSD methodology back-port

This finding's pattern + recovery is documented in:

- `adsd-publish/case-study/cobrust-studio-experience.md` §11.3
  (Two-phase SOP M6 implementation bug case study)
- `adsd-publish/reference/failure-modes-catalogue.md` F1.5
  (test-corpus structural blind spot on re-derive paths)

Both landed in ADSD methodology v1.2.7 at commit `ccdeb19` in the
`Cobrust-lang/agent-driven-development` repo.

## Cross-references

- ADR-0007 §"Wire format" (the implicit-vs-explicit contract that
  this finding made explicit)
- src: `crates/studio-server/src/secret.rs::seal()` (pre-fix:
  fresh-random salt; post-fix: `self.salt`)
- src: `crates/studio-server/tests/secret_roundtrip.rs`
  (login-aead test 2 was the original repro)
- src: `crates/studio-server/src/secret.rs::tests::seal_then_re_derive_then_open_round_trips`
  (regression lock)
- ADSD failure-modes-catalogue v1.2.7 F1.5 (back-ported pattern)
- ADSD case study cobrust-studio-experience.md §11.3 (full
  walk-through)
- CHANGELOG.md [0.3.0] §Fixed (release notes summary)
