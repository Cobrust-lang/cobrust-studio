//! M6 secret-storage AEAD round-trip — integration test skeleton.
//!
//! Per ADR-0007 §"Done means" item 2, three integration scenarios
//! gate M6 closure. Each is currently `#[ignore]`-attributed with
//! an `unimplemented!()` body so the file compiles (so the broader
//! workspace `cargo test --workspace --locked` gate stays green
//! during Phase 1) AND the Phase 2 P9 sub-agent has a clear test
//! target to un-ignore + implement.
//!
//! Run only the un-ignored M6 tests after Phase 2 lands with:
//!   cargo test -p studio-server --test secret_roundtrip
//!
//! ADR-0007 binds the algorithm + wire format (AES-256-GCM +
//! Argon2id, packed `salt(16) || nonce(12) || ciphertext+tag`
//! under scheme tag `"aes-gcm-256/argon2id-v1"`). Tests assert
//! that the deployed module honors that pin.

#![allow(clippy::unwrap_used, clippy::expect_used)]

/// POST /api/login with `(endpoint, api_key, model, passphrase)`,
/// then dispatch a request using the in-memory `SessionKey`. Asserts
/// the dispatch resolves to the decrypted endpoint+key without ever
/// reading from `ANTHROPIC_API_KEY` env var.
///
/// This is the **primary regression gate** for M6 — proves the
/// env-var workaround is no longer required for the happy path.
/// Aligns with ADR-0007 §"Done means" item 2 sub-bullet 1.
#[ignore = "Phase 2 P9 unblocks — ADR-0007 M6 round-trip not yet implemented"]
#[tokio::test]
async fn login_then_dispatch_with_in_memory_key() {
    unimplemented!(
        "ADR-0007 Phase 2 P9 deliverable: \
         POST /api/login → derived SessionKey held in app state → \
         dispatch round-trips via session_kv-decrypt path, no env var."
    );
}

/// POST /api/login → simulate process restart by constructing a new
/// app-state instance (no `SessionKey` carryover) → next dispatch
/// returns `401 NoSession`.
///
/// Verifies the "binary restart drops in-memory key" property in
/// ADR-0007 §"Decision" sub-bullet 6 and §"Done means" item 2
/// sub-bullet 2. Distinguishes "session_kv blob still on disk"
/// from "decrypted key still in process memory" — the cold-disk-
/// theft threat-model attacker (in-scope #1) reads the blob but
/// has no key.
#[ignore = "Phase 2 P9 unblocks — ADR-0007 M6 round-trip not yet implemented"]
#[tokio::test]
async fn restart_drops_key_returns_401() {
    unimplemented!(
        "ADR-0007 Phase 2 P9 deliverable: \
         restart binary → dispatch returns 401 NoSession; \
         session_kv blob persists; passphrase re-entry re-derives."
    );
}

/// POST /api/login with mismatched passphrase against existing
/// session_kv blob → AEAD tag validation fails → `401`.
///
/// Verifies AEAD authenticity (not just confidentiality) per
/// ADR-0007 §"Algorithm choice" AES-256-GCM tag check. Aligns with
/// §"Done means" item 2 sub-bullet 3 + item 1 sub-bullet
/// `wrong_passphrase_fails_open`.
#[ignore = "Phase 2 P9 unblocks — ADR-0007 M6 round-trip not yet implemented"]
#[tokio::test]
async fn wrong_passphrase_login_returns_401() {
    unimplemented!(
        "ADR-0007 Phase 2 P9 deliverable: \
         POST /api/login with mismatched passphrase against \
         existing blob → SecretError::Open → HTTP 401."
    );
}
