//! Session encrypted-blob KV — Wave A2 TDD red.
//!
//! Contract per `docs/agent/modules/studio-store.md` §"Public surface":
//!   - `session::set_endpoint(blob: EncryptedBlob)`
//!   - `session::get_endpoint() -> Option<EncryptedBlob>`
//!
//! `EncryptedBlob` is opaque to studio-store — this corpus tests storage only.
//! Encryption format / cipher is out of scope (lives in studio-server per
//! ADR-0003).

mod common;

use studio_store::Store;
use studio_store::session::EncryptedBlob;

use common::fresh_studio_root;

/// Fresh store: `get_endpoint` returns `None`.
#[tokio::test]
async fn get_endpoint_on_empty_returns_none() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");
    let got = store
        .session()
        .get_endpoint()
        .await
        .expect("get_endpoint OK");
    assert!(
        got.is_none(),
        "fresh store must have no endpoint; got {got:?}"
    );
}

/// `set_endpoint` then `get_endpoint` returns the same bytes.
#[tokio::test]
async fn set_then_get_endpoint_roundtrips() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");
    let blob = EncryptedBlob::from(vec![0x01, 0x02, 0xff, 0xab, 0xcd]);
    store
        .session()
        .set_endpoint(blob.clone())
        .await
        .expect("set_endpoint OK");
    let got = store
        .session()
        .get_endpoint()
        .await
        .expect("get_endpoint OK")
        .expect("just-set endpoint must be present");
    assert_eq!(
        got.as_bytes(),
        blob.as_bytes(),
        "set/get must preserve every byte (EncryptedBlob is opaque)"
    );
}

/// Second `set_endpoint` overwrites the first (key/value semantics).
#[tokio::test]
async fn set_endpoint_overwrites_prior_value() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");

    let first = EncryptedBlob::from(vec![1, 2, 3]);
    let second = EncryptedBlob::from(vec![9, 9, 9, 9]);

    store
        .session()
        .set_endpoint(first)
        .await
        .expect("first set");
    store
        .session()
        .set_endpoint(second.clone())
        .await
        .expect("second set");

    let got = store
        .session()
        .get_endpoint()
        .await
        .expect("get")
        .expect("endpoint present");
    assert_eq!(
        got.as_bytes(),
        second.as_bytes(),
        "second set must overwrite first; got {:?}",
        got.as_bytes()
    );
}

/// Endpoint persists across `Store::open` re-instantiation (SQLite-backed).
#[tokio::test]
async fn endpoint_survives_store_reopen() {
    let (_guard, root) = fresh_studio_root();
    let blob = EncryptedBlob::from(vec![0xde, 0xad, 0xbe, 0xef]);
    {
        let store = Store::open(&root).await.expect("Store::open");
        store
            .session()
            .set_endpoint(blob.clone())
            .await
            .expect("set");
    }
    // New Store handle on same root.
    let store2 = Store::open(&root).await.expect("re-open Store");
    let got = store2
        .session()
        .get_endpoint()
        .await
        .expect("get")
        .expect("endpoint must persist across reopen");
    assert_eq!(got.as_bytes(), blob.as_bytes());
}
