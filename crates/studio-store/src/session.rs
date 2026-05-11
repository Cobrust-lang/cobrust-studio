//! Session k/v store — encrypted endpoint blob.
//!
//! Per ADR-0003 §"Decision", API keys never live on disk in plaintext.
//! studio-store doesn't perform the encryption itself (that's the job
//! of studio-server's auth layer, which holds the user's passphrase or
//! per-session key); we store the opaque `(ciphertext, nonce, scheme)`
//! triple keyed by a string slot id.
//!
//! M1 only uses the `"endpoint"` slot. Extra slots are reserved for
//! future use (e.g. per-provider credentials when multi-provider lands).

use serde::{Deserialize, Serialize};

use crate::Store;
use crate::error::StoreError;

/// Slot id for the primary endpoint credential.
pub const SLOT_ENDPOINT: &str = "endpoint";

/// Opaque encrypted credential blob.
///
/// `ciphertext` and `nonce` are scheme-defined byte strings;
/// `scheme` is a free-form tag the auth layer interprets (e.g.
/// `"aes-gcm-256/argon2id"`). studio-store does NOT decrypt; it is a
/// pass-through to the caller.
///
/// Implements `From<Vec<u8>>` for callers that hold raw opaque bytes (no
/// scheme/nonce split yet — `scheme = "raw"`, `nonce = vec![]`); callers
/// constructing the AEAD triple use the explicit struct literal instead.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncryptedBlob {
    /// Encrypted ciphertext bytes.
    pub ciphertext: Vec<u8>,
    /// Per-blob nonce / IV bytes.
    pub nonce: Vec<u8>,
    /// Caller-defined scheme tag.
    pub scheme: String,
}

impl EncryptedBlob {
    /// View the ciphertext bytes (the opaque payload).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.ciphertext
    }
}

impl From<Vec<u8>> for EncryptedBlob {
    fn from(bytes: Vec<u8>) -> Self {
        Self {
            ciphertext: bytes,
            nonce: Vec::new(),
            scheme: "raw".to_string(),
        }
    }
}

/// Sub-handle returned by [`Store::session`].
#[derive(Debug)]
pub struct SessionHandle<'a> {
    store: &'a Store,
}

impl<'a> SessionHandle<'a> {
    pub(crate) const fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// Persist `blob` under the `"endpoint"` slot, replacing any prior
    /// value.
    ///
    /// # Errors
    /// SQLite errors bubble up.
    pub async fn set_endpoint(&self, blob: EncryptedBlob) -> Result<(), StoreError> {
        self.set(SLOT_ENDPOINT, &blob).await
    }

    /// Fetch the `"endpoint"` slot, or `None` when unset.
    ///
    /// # Errors
    /// SQLite errors bubble up.
    pub async fn get_endpoint(&self) -> Result<Option<EncryptedBlob>, StoreError> {
        self.get(SLOT_ENDPOINT).await
    }

    /// Generic slot writer.
    ///
    /// # Errors
    /// SQLite errors bubble up.
    pub async fn set(&self, key: &str, blob: &EncryptedBlob) -> Result<(), StoreError> {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
        sqlx::query(
            "INSERT INTO session_kv (key, value, nonce, scheme, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(key) DO UPDATE SET
                value=excluded.value,
                nonce=excluded.nonce,
                scheme=excluded.scheme,
                updated_at=excluded.updated_at",
        )
        .bind(key)
        .bind(&blob.ciphertext)
        .bind(&blob.nonce)
        .bind(&blob.scheme)
        .bind(now)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    /// Generic slot reader.
    ///
    /// # Errors
    /// SQLite errors bubble up.
    pub async fn get(&self, key: &str) -> Result<Option<EncryptedBlob>, StoreError> {
        let row: Option<(Vec<u8>, Vec<u8>, String)> =
            sqlx::query_as("SELECT value, nonce, scheme FROM session_kv WHERE key = ?")
                .bind(key)
                .fetch_optional(self.store.pool())
                .await?;
        Ok(row.map(|(ciphertext, nonce, scheme)| EncryptedBlob {
            ciphertext,
            nonce,
            scheme,
        }))
    }

    /// Drop a slot. Idempotent.
    ///
    /// # Errors
    /// SQLite errors bubble up.
    pub async fn remove(&self, key: &str) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM session_kv WHERE key = ?")
            .bind(key)
            .execute(self.store.pool())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn sample_blob() -> EncryptedBlob {
        EncryptedBlob {
            ciphertext: vec![0xde, 0xad, 0xbe, 0xef],
            nonce: vec![
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
            ],
            scheme: "aes-gcm-256/argon2id".into(),
        }
    }

    #[tokio::test]
    async fn endpoint_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).await.unwrap();
        assert!(store.session().get_endpoint().await.unwrap().is_none());

        let blob = sample_blob();
        store.session().set_endpoint(blob.clone()).await.unwrap();
        let got = store.session().get_endpoint().await.unwrap().unwrap();
        assert_eq!(got, blob);
    }

    #[tokio::test]
    async fn set_endpoint_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).await.unwrap();
        let mut blob = sample_blob();
        store.session().set_endpoint(blob.clone()).await.unwrap();
        blob.scheme = "v2".into();
        store.session().set_endpoint(blob.clone()).await.unwrap();
        let got = store.session().get_endpoint().await.unwrap().unwrap();
        assert_eq!(got.scheme, "v2");
    }

    #[tokio::test]
    async fn remove_drops_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).await.unwrap();
        store.session().set_endpoint(sample_blob()).await.unwrap();
        store.session().remove(SLOT_ENDPOINT).await.unwrap();
        assert!(store.session().get_endpoint().await.unwrap().is_none());
    }
}
