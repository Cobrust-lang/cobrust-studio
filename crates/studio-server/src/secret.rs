//! AEAD secret-storage module — ADR-0007 M6.
//!
//! Implements the server-side AES-256-GCM + Argon2id key-derivation + seal/open
//! round-trip described in ADR-0007 §"Decision".
//!
//! ## Wire format
//!
//! The `ciphertext` field of [`studio_store::session::EncryptedBlob`] stores:
//!
//! ```text
//! <16-byte salt> || <12-byte nonce> || <AES-256-GCM ciphertext+tag>
//! ```
//!
//! under `scheme = "aes-gcm-256/argon2id-v1"`. The `nonce` column in
//! `session_kv` is left empty (empty `Vec`); all per-blob keying material is
//! packed into `ciphertext` for schema-compat with the v0.1.0 layout.
//!
//! ## Argon2id parameters (locked to `-v1` scheme tag)
//!
//! | param  | value  | rationale |
//! |--------|--------|-----------|
//! | m_cost | 65536  | 64 MiB memory — OWASP 2025 interactive-auth minimum |
//! | t_cost | 3      | 3 iterations |
//! | p_cost | 1      | single-lane (single-user MVP) |
//! | output | 32 B   | AES-256 key size |
//!
//! The `-v1` suffix is a cryptographic suite revision marker. Parameter bumps
//! land as `-v2`, `-v3`, etc.; old blobs remain readable via their scheme tag.
//!
//! ## Threat model
//!
//! In-scope (ADR-0007 §"Threat model"):
//! - Cold-disk theft: attacker has the `session_kv` blob but not the passphrase.
//!   Without the passphrase the blob is opaque AES-GCM ciphertext.
//!
//! Out-of-scope (documented in ADR-0007; not solved here):
//! - Running-process memory dump (in-memory `SessionKey` is plaintext).
//! - Stop-and-restart attack (TLS is the operator's responsibility).
//! - Multi-user / per-tenant key separation (deferred to v0.3.x+).

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use argon2::Argon2;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

// Re-export so `routes/login.rs` and `routes/dispatch.rs` can use
// `crate::secret::ProviderKind` without crossing the studio-router boundary
// awkwardly, while tests in this crate import it from one place.
pub use studio_router::ProviderKind;

/// Scheme tag written to `session_kv.scheme` for this module's blobs.
/// The `-v1` suffix locks the Argon2id parameter set (ADR-0007 §"Decision").
pub const SCHEME: &str = "aes-gcm-256/argon2id-v1";

/// Byte offset constants for the packed blob layout.
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = SALT_LEN + NONCE_LEN;
/// Minimum ciphertext size: header + 16-byte GCM authentication tag.
const MIN_BLOB_LEN: usize = HEADER_LEN + 16;

/// Per-session AES-256 key derived from the user's passphrase.
///
/// Held in-process in `AppState.session_key: Arc<RwLock<Option<SessionKey>>>`
/// for the lifetime of the binary. Dropped on logout or process restart.
/// Cloning is intentional — the 32-byte key is small and the `RwLock` read
/// path clones it out so subsequent dispatches do not hold the lock during
/// crypto operations.
/// AEAD session key + the salt it was derived with.
///
/// `seal()` must pack `self.salt` into the blob (not a fresh random salt) so
/// that a subsequent `derive(passphrase, blob[..16])` reproduces this same
/// key — that's the round-trip path login.rs takes on the wrong-passphrase
/// check + on binary restart.
#[derive(Clone)]
pub struct SessionKey {
    key: [u8; 32],
    salt: [u8; 16],
}

impl std::fmt::Debug for SessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the key bytes. Debug renders only a redacted marker.
        f.debug_struct("SessionKey")
            .field("key", &"[REDACTED]")
            .field("salt", &"[REDACTED]")
            .finish()
    }
}

impl SessionKey {
    /// Derive a `SessionKey` from `passphrase` + `salt` using Argon2id.
    ///
    /// Parameters are fixed by the `-v1` scheme revision marker.
    /// Same passphrase + same salt yields the same key (deterministic).
    ///
    /// # Errors
    /// Returns [`SecretError::Kdf`] if the Argon2id computation fails
    /// (e.g. invalid parameter configuration — should not happen with the
    /// fixed params, but propagated for correctness).
    pub fn derive(passphrase: &str, salt: &[u8; 16]) -> Result<Self, SecretError> {
        // ADR-0007 §"Argon2id parameters": m=64MiB, t=3, p=1, output=32B.
        let params = argon2::Params::new(
            65_536,   // m_cost = 64 MiB
            3,        // t_cost
            1,        // p_cost
            Some(32), // output len
        )
        .map_err(SecretError::Kdf)?;
        let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        let mut key_bytes = [0u8; 32];
        argon2
            .hash_password_into(passphrase.as_bytes(), salt, &mut key_bytes)
            .map_err(SecretError::Kdf)?;
        Ok(Self {
            key: key_bytes,
            salt: *salt,
        })
    }

    /// Seal `payload` into the packed `salt || nonce || ciphertext+tag` wire
    /// format (ADR-0007 §"Decision" §"Storage wire format").
    ///
    /// Packs **`self.salt`** (the salt this key was derived with) into the
    /// blob header — NOT a fresh random salt — so a subsequent
    /// `SessionKey::derive(passphrase, blob[..16])` reproduces this exact
    /// key. The 12-byte nonce IS freshly random per seal (AES-GCM requires
    /// nonce uniqueness; reusing nonces would catastrophically break the
    /// cipher).
    ///
    /// # Errors
    /// Returns [`SecretError::Seal`] if the AEAD encryption fails.
    pub fn seal(&self, payload: &EndpointSecret) -> Result<Vec<u8>, SecretError> {
        let json = serde_json::to_vec(payload)
            .map_err(|e| SecretError::Seal(format!("json encode: {e}")))?;

        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);

        let key = Key::<Aes256Gcm>::from_slice(&self.key);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, json.as_slice())
            .map_err(|e| SecretError::Seal(e.to_string()))?;

        // Pack: salt(16) || nonce(12) || ciphertext+tag
        // Salt is self.salt — the same salt this key was derived with, so
        // RE-DERIVE on next login (read blob[..16], derive(passphrase, that))
        // reproduces self exactly.
        let mut blob = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        blob.extend_from_slice(&self.salt);
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);
        Ok(blob)
    }

    /// Seal raw JSON bytes into the packed `salt || nonce || ciphertext+tag`
    /// wire format without going through [`EndpointSecret`] serialisation.
    ///
    /// Used by integration tests to simulate a pre-M7 binary that seals a
    /// JSON payload that has no `provider_kind` field, verifying the
    /// `#[serde(default)]` back-compat path when `open()` decrypts it.
    ///
    /// # Errors
    /// Returns [`SecretError::Seal`] if the AEAD encryption fails.
    #[doc(hidden)]
    pub fn seal_raw(&self, json_bytes: &[u8]) -> Result<Vec<u8>, SecretError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);

        let key = Key::<Aes256Gcm>::from_slice(&self.key);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, json_bytes)
            .map_err(|e| SecretError::Seal(e.to_string()))?;

        let mut blob = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        blob.extend_from_slice(&self.salt);
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);
        Ok(blob)
    }

    /// Open and decrypt a packed blob produced by [`Self::seal`].
    ///
    /// The blob must be at least [`MIN_BLOB_LEN`] bytes. The salt is
    /// extracted from the blob, but the key was derived BEFORE calling `open`
    /// (the caller holds the in-memory `SessionKey` derived at login). This
    /// means a tampered salt causes the key to mismatch and the GCM tag check
    /// fails with [`SecretError::Open`].
    ///
    /// # Errors
    /// - [`SecretError::Malformed`] — blob shorter than the minimum.
    /// - [`SecretError::Open`] — AEAD authentication failure (wrong key, bit
    ///   flip in ciphertext or nonce, wrong salt used at derive time).
    pub fn open(&self, blob: &[u8]) -> Result<EndpointSecret, SecretError> {
        if blob.len() < MIN_BLOB_LEN {
            return Err(SecretError::Malformed {
                expected: MIN_BLOB_LEN,
                got: blob.len(),
            });
        }

        let nonce_bytes = &blob[SALT_LEN..HEADER_LEN];
        let ciphertext = &blob[HEADER_LEN..];

        let key = Key::<Aes256Gcm>::from_slice(&self.key);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| SecretError::Open(e.to_string()))?;

        serde_json::from_slice::<EndpointSecret>(&plaintext)
            .map_err(|e| SecretError::Open(format!("json decode: {e}")))
    }
}

/// Plaintext credential payload stored encrypted in `session_kv`.
///
/// Serialized as JSON before AES-GCM encryption; deserialized after
/// decryption. The `model` field enables the per-dispatch provider
/// construction without a separate config lookup.
///
/// ## M7 addition (ADR-0008)
///
/// `provider_kind` is `#[serde(default)]` so pre-M7 sealed blobs (which
/// have no `provider_kind` field) deserialize with `Anthropic`, matching
/// the v0.2.x implicit behaviour.
///
/// ## Aleksandr v3 P1
///
/// `Debug` is hand-written to redact `api_key` (production secret). A
/// derived `Debug` on this struct would silently spray decrypted
/// credentials into structured logs if any caller ever did
/// `tracing::instrument` or `tracing::debug!("{:?}", secret)`.
#[derive(Clone, Serialize, Deserialize)]
pub struct EndpointSecret {
    /// LLM provider base URL (e.g. `"https://api.anthropic.com"`).
    pub endpoint: String,
    /// API key — never stored plaintext, always sealed. Never logged.
    pub api_key: String,
    /// Model identifier (e.g. `"claude-opus-4-7"`).
    pub model: String,
    /// Provider API kind — `Anthropic` by default for v0.2.x back-compat.
    #[serde(default)]
    pub provider_kind: ProviderKind,
}

impl std::fmt::Debug for EndpointSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Redact api_key; the other fields are not secrets but stay terse.
        f.debug_struct("EndpointSecret")
            .field("endpoint", &self.endpoint)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("provider_kind", &self.provider_kind)
            .finish()
    }
}

/// Errors from the secret-storage module (ADR-0007 §"Decision").
///
/// `#[non_exhaustive]` so future variants (e.g. when v2+ scheme support
/// lands) don't break downstream `match` arms — Aleksandr v3 audit P3 #5.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SecretError {
    /// Argon2id key-derivation failure.
    #[error("argon2id key derivation failed: {0}")]
    Kdf(argon2::Error),

    /// AES-GCM encryption failure.
    #[error("aead seal failed: {0}")]
    Seal(String),

    /// AES-GCM decryption or authentication failure (wrong key, tampered blob).
    #[error("aead open failed: {0}")]
    Open(String),

    /// Blob is too short to hold the expected header + minimum ciphertext.
    #[error("malformed blob: expected ≥{expected} bytes, got {got}")]
    Malformed {
        /// Expected minimum byte count.
        expected: usize,
        /// Actual byte count.
        got: usize,
    },

    /// Scheme tag is not recognised by this version of Studio.
    /// Reserved for the future scheme-guard at the `EncryptedBlob` boundary.
    ///
    /// **Currently not produced by any function in this module.** The login
    /// handler in `routes/login.rs` performs `blob.scheme == SCHEME` checks
    /// inline and falls through to either the existing-blob path or fresh-
    /// login overwrite. When v2+ schemes land (future Argon2id parameter
    /// rev, or a chacha20poly1305 variant), the scheme-discriminator logic
    /// moves here as `EndpointSecret::validate_scheme(&str) -> Result<...,
    /// SecretError::UnknownScheme>` and login.rs delegates to it. Keeping
    /// the variant reserved now means that addition is non-breaking for
    /// downstream `match` arms (also why this enum is `#[non_exhaustive]`
    /// — Aleksandr v3 audit P3 #5).
    #[allow(dead_code)] // Reserved-for-future-scheme-guard — see doc above.
    #[error("unknown scheme: {0}")]
    UnknownScheme(String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Verify Argon2id is deterministic: same passphrase + same salt → same key.
    #[test]
    fn argon2id_kdf_deterministic() {
        let passphrase = "correct horse battery staple";
        let salt = [0x42u8; 16];
        let k1 = SessionKey::derive(passphrase, &salt).unwrap();
        let k2 = SessionKey::derive(passphrase, &salt).unwrap();
        assert_eq!(k1.key, k2.key, "same inputs must yield same key");
        assert_eq!(k1.salt, k2.salt, "same inputs must yield same salt");
    }

    /// Seal then RE-DERIVE then open — the round-trip login.rs actually uses.
    ///
    /// Regression test for the M6 wrong-passphrase-false-positive bug: seal()
    /// must pack `self.salt` into the blob so `derive(passphrase, blob[..16])`
    /// reproduces the same key. The original P9 implementation generated a
    /// fresh random salt inside seal() → packed_salt did NOT match
    /// derive_salt → re-derive produced a different key → open failed → all
    /// subsequent logins with the correct passphrase reported wrong_passphrase.
    #[test]
    fn seal_then_re_derive_then_open_round_trips() {
        let passphrase = "re-derive-test-m6";
        let salt = [0x77u8; 16];
        let key1 = SessionKey::derive(passphrase, &salt).unwrap();
        let secret = EndpointSecret {
            endpoint: "https://api.anthropic.com".to_string(),
            api_key: "sk-rederive".to_string(),
            model: "claude-opus-4-7".to_string(),
            provider_kind: ProviderKind::Anthropic,
        };
        let blob = key1.seal(&secret).unwrap();

        // The packed salt MUST equal the derive salt (the whole point of the fix).
        assert_eq!(&blob[..16], &salt, "packed salt must equal derive salt");

        // Simulate the login.rs wrong-passphrase-check / restart-recovery path:
        // read salt from blob, re-derive, open.
        let mut blob_salt = [0u8; 16];
        blob_salt.copy_from_slice(&blob[..16]);
        let key2 = SessionKey::derive(passphrase, &blob_salt).unwrap();
        assert_eq!(key1.key, key2.key, "re-derive must reproduce key");

        let recovered = key2.open(&blob).expect("re-derived key must open");
        assert_eq!(recovered.endpoint, secret.endpoint);
        assert_eq!(recovered.api_key, secret.api_key);
        assert_eq!(recovered.model, secret.model);
    }

    /// Verify the full AES-GCM round-trip: seal then open returns the original.
    #[test]
    fn aes_gcm_round_trip() {
        let passphrase = "test-passphrase-m6";
        let salt = [0x11u8; 16];
        let key = SessionKey::derive(passphrase, &salt).unwrap();
        let secret = EndpointSecret {
            endpoint: "https://api.anthropic.com".to_string(),
            api_key: "sk-ant-test-key".to_string(),
            model: "claude-opus-4-7".to_string(),
            provider_kind: ProviderKind::Anthropic,
        };
        let blob = key.seal(&secret).unwrap();
        let recovered = key.open(&blob).unwrap();
        assert_eq!(recovered.endpoint, secret.endpoint);
        assert_eq!(recovered.api_key, secret.api_key);
        assert_eq!(recovered.model, secret.model);
    }

    /// Verify that a different passphrase fails to open the blob.
    #[test]
    fn wrong_passphrase_fails_open() {
        let salt = [0x22u8; 16];
        let correct_key = SessionKey::derive("correct-pass", &salt).unwrap();
        let wrong_key = SessionKey::derive("wrong-pass", &salt).unwrap();
        let secret = EndpointSecret {
            endpoint: "https://api.example.com".to_string(),
            api_key: "sk-secret".to_string(),
            model: "model-x".to_string(),
            provider_kind: ProviderKind::Anthropic,
        };
        let blob = correct_key.seal(&secret).unwrap();
        let result = wrong_key.open(&blob);
        assert!(
            matches!(result, Err(SecretError::Open(_))),
            "wrong passphrase must fail with SecretError::Open, got: {result:?}",
        );
    }

    /// Verify that flipping a byte in the ciphertext region fails AEAD tag check.
    #[test]
    fn tampered_ciphertext_fails_open() {
        let salt = [0x33u8; 16];
        let key = SessionKey::derive("tamper-test", &salt).unwrap();
        let secret = EndpointSecret {
            endpoint: "https://api.anthropic.com".to_string(),
            api_key: "sk-tamper".to_string(),
            model: "model-y".to_string(),
            provider_kind: ProviderKind::Anthropic,
        };
        let mut blob = key.seal(&secret).unwrap();
        // Flip the last byte of the ciphertext (within the GCM tag region).
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        let result = key.open(&blob);
        assert!(
            matches!(result, Err(SecretError::Open(_))),
            "tampered ciphertext must fail, got: {result:?}",
        );
    }

    /// Verify that flipping the blob's salt causes the re-derived key to
    /// mismatch and the GCM tag check to fail on open.
    ///
    /// Scenario: attacker flips the salt bytes stored in `session_kv`. After
    /// a process restart, the user re-enters their passphrase; the server
    /// re-derives the key from the **tampered** salt. The derived key is wrong
    /// (different salt → different key), so the AEAD tag check fails.
    ///
    /// This test models the ADR-0007 §"Done means" scenario by simulating the
    /// re-derive step: extract salt from blob, flip it, re-derive, then open.
    #[test]
    fn tampered_salt_fails_open() {
        let salt = [0x44u8; 16];
        let passphrase = "salt-test";
        let key = SessionKey::derive(passphrase, &salt).unwrap();
        let secret = EndpointSecret {
            endpoint: "https://api.example.com".to_string(),
            api_key: "sk-salt".to_string(),
            model: "model-z".to_string(),
            provider_kind: ProviderKind::Anthropic,
        };
        let mut blob = key.seal(&secret).unwrap();
        // Flip the first byte of the embedded salt in the blob.
        blob[0] ^= 0xFF;
        // Re-derive from the TAMPERED salt (simulates process restart where
        // the server reads the salt from the stored blob before re-deriving).
        let mut tampered_salt = [0u8; 16];
        tampered_salt.copy_from_slice(&blob[..16]);
        let wrong_key = SessionKey::derive(passphrase, &tampered_salt).unwrap();
        // The key derived from the tampered salt is different from the original
        // key that produced the ciphertext, so the AEAD tag fails.
        let result = wrong_key.open(&blob);
        assert!(
            matches!(result, Err(SecretError::Open(_))),
            "tampered salt must yield wrong key → AEAD tag fail, got: {result:?}",
        );
    }

    /// ADR-0008 M7 back-compat: deserialize a pre-M7 JSON payload (no
    /// `provider_kind` field) → provider_kind defaults to `Anthropic`.
    #[test]
    fn endpoint_secret_back_compat() {
        let json = r#"{"endpoint":"https://api.anthropic.com","api_key":"sk-test","model":"claude-opus-4-7"}"#;
        let secret: EndpointSecret = serde_json::from_str(json).expect("deserialize");
        assert_eq!(
            secret.provider_kind,
            ProviderKind::Anthropic,
            "missing provider_kind must default to Anthropic"
        );
        assert_eq!(secret.endpoint, "https://api.anthropic.com");
    }

    /// ADR-0008 M7 forward-compat: serialize an `EndpointSecret` with
    /// `provider_kind: Openai` → deserialize → round-trip preserves the kind.
    #[test]
    fn endpoint_secret_forward_compat() {
        let original = EndpointSecret {
            endpoint: "https://api.openai.com/v1".to_string(),
            api_key: "sk-openai".to_string(),
            model: "gpt-5".to_string(),
            provider_kind: ProviderKind::Openai,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: EndpointSecret = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            recovered.provider_kind,
            ProviderKind::Openai,
            "Openai kind must round-trip"
        );
        assert_eq!(recovered.endpoint, original.endpoint);
        assert_eq!(recovered.api_key, original.api_key);
        assert_eq!(recovered.model, original.model);
    }

    /// Verify that a too-short blob returns `SecretError::Malformed`.
    /// Argon2id wall-clock benchmark — Sarah v3 audit "low-end laptop ≤
    /// 2s" gate. Ignored by default (timing-dependent + slow on emulated
    /// targets). Run explicitly with:
    ///
    ///   cargo test -p studio-server --release secret::tests::bench_argon2id_derive -- --ignored --nocapture
    ///
    /// Records the wall-clock of N=5 sequential derives (same params as
    /// production: m=64MiB, t=3, p=1, output=32B). Asserts the **median**
    /// stays under the ADR-0007 §"Performance" tolerance (≤ 2 seconds —
    /// the upper bound at which interactive login UX starts feeling
    /// stuck). ADR-0007 §"Done means" item 6 anchored ~500ms on a
    /// 2020-era laptop; the 2s ceiling gives headroom for CI runners +
    /// older Intel hardware.
    ///
    /// **Caveat**: in `#[cfg(test)]` (debug profile) Argon2id is 3-5×
    /// slower than `--release`. The 2s ceiling applies to release-mode
    /// runs; debug runs may exceed it without indicating a real
    /// regression. The `--release` flag in the invocation above is
    /// mandatory for this gate.
    #[ignore = "release-mode timing benchmark; run explicitly with --ignored --nocapture"]
    #[test]
    fn bench_argon2id_derive() {
        use std::time::Instant;

        const N: usize = 5;
        const CEILING_MS: u128 = 2_000;

        let passphrase = "bench-test-passphrase-2026";
        let salt = [0xA5u8; 16];

        // Warm-up — JIT + page-fault cost shouldn't dominate the first
        // measurement.
        let _warmup = SessionKey::derive(passphrase, &salt).unwrap();

        let mut samples_ms: Vec<u128> = Vec::with_capacity(N);
        for i in 0..N {
            let t0 = Instant::now();
            let _k = SessionKey::derive(passphrase, &salt).unwrap();
            let elapsed = t0.elapsed();
            let ms = elapsed.as_millis();
            samples_ms.push(ms);
            println!("  sample {}: {} ms", i + 1, ms);
        }

        samples_ms.sort_unstable();
        let median_ms = samples_ms[N / 2];
        let min_ms = samples_ms[0];
        let max_ms = samples_ms[N - 1];

        println!(
            "\nArgon2id derive (m=64MiB, t=3, p=1, out=32B) — N={N}:\n  \
             min={min_ms} ms  median={median_ms} ms  max={max_ms} ms"
        );

        // Hard ceiling — ADR-0007 + Sarah v3 v0.3.x gate. Release mode.
        // Debug mode may legitimately exceed this; the #[ignore] gate
        // is the discipline boundary.
        assert!(
            median_ms <= CEILING_MS,
            "Argon2id derive median {median_ms} ms exceeds {CEILING_MS} ms ceiling \
             (ADR-0007 §Performance; Sarah v3 v0.3.x pilot gate). Re-tune \
             m_cost / t_cost / p_cost in `SessionKey::derive` if persistent.",
        );
    }

    #[test]
    fn malformed_blob_too_short() {
        let salt = [0x55u8; 16];
        let key = SessionKey::derive("malformed-test", &salt).unwrap();
        // Only 10 bytes — far less than the 44-byte minimum.
        let short_blob = vec![0u8; 10];
        let result = key.open(&short_blob);
        assert!(
            matches!(
                result,
                Err(SecretError::Malformed {
                    expected: MIN_BLOB_LEN,
                    got: 10
                })
            ),
            "too-short blob must return Malformed, got: {result:?}",
        );
    }
}
