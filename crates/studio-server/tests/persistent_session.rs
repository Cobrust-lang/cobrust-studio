//! M8 persistent-session — integration tests (ADR-0009 Phase 2).
//!
//! Per ADR-0009 §"Done means" item 2, four integration scenarios gate
//! M8 closure. Each test exercises the full HTTP round-trip through
//! the Axum app + simulates a binary restart by constructing a fresh
//! `AppState` with the SAME `Store` + persist backend.
//!
//! Run only the M8 tests with:
//!   cargo test -p studio-server --test persistent_session
//!
//! ADR-0009 binds the algorithm + boot-flow (passphrase wrap → boot
//! reads passphrase → `SessionKey::derive` → verify via `open()` →
//! stash in `AppState.session_key`). Tests assert that the deployed
//! module honours that pin AND the F1.5 lesson (test the path the
//! caller actually walks, not just a same-instance round-trip).
//!
//! ## Coverage matrix
//!
//! | Test | Backend | Scenario |
//! |---|---|---|
//! | `file_persist_path_survives_restart` | File | Happy path: login → restart → session_key restored |
//! | `none_persist_path_drops_key_on_restart` | None | v0.3.0 baseline: restart drops key (regression gate) |
//! | `logout_purge_clears_file_persist` | File | `?purge=true` deletes the file |
//! | `wrong_persist_passphrase_invalidates_and_clears` | File | Orphaned/stale persist auto-clears on boot |
//! | `keychain_path_survives_restart` | Keychain | Same as file_path but via OS keychain (#[ignore]'d for CI) |

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::items_after_statements
)]

mod common;

use std::sync::Arc;

use std::io;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::oneshot_post_json;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use studio_router::{Router as LlmRouter, RouterBuilder, RouterConfig};
use studio_server::persist::{FileStore, KeychainStore, NullStore, PersistError, PersistStore};
use studio_server::{AppState, SyntheticProvider, build_router};
use studio_store::Store;
use tower::ServiceExt;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a synthetic fallback router (never hits the network).
async fn synthetic_router(root: &std::path::Path) -> Arc<LlmRouter> {
    let cache_dir = root.join("llm_cache");
    let ledger = root.join("ledger.jsonl");
    let cache_toml = cache_dir.to_string_lossy().replace('\\', "/");
    let ledger_toml = ledger.to_string_lossy().replace('\\', "/");
    let toml = format!(
        r#"
[router]
strategy = "quality"
cache_dir = "{cache_toml}"
ledger_path = "{ledger_toml}"
preferred = ["synth:synthetic-1"]

[providers.synth]
kind = "synthetic"
base_url = "https://example.invalid"
api_key_env = ""
models = ["synthetic-1"]
"#,
    );
    let cfg = RouterConfig::from_toml_str(&toml).expect("config parse");
    let provider = Arc::new(SyntheticProvider::new("synth"));
    let router: LlmRouter = RouterBuilder::new()
        .register_provider("synth", provider)
        .build(&cfg)
        .await
        .expect("router build");
    Arc::new(router)
}

/// Boot a fresh `AppState` with the supplied persist backend, mimicking
/// what `serve()` does at startup (minus the actual TCP bind). Returns
/// `(AppState, axum::Router)` so tests can drive both the router and
/// inspect the in-memory session_key.
async fn boot_with_persist(
    root: std::path::PathBuf,
    persist: Arc<dyn PersistStore + Send + Sync>,
) -> (AppState, axum::Router) {
    let store = Store::open(&root).await.expect("Store::open");
    let static_router = synthetic_router(&root).await;
    let state = AppState::with_persist(store, Some(static_router), root, persist);
    let app = build_router(state.clone());
    (state, app)
}

/// Run the M8 boot-flow auto-unlock against `state` (i.e. simulate
/// what `studio_server::serve()` does at boot when the persist backend
/// is non-None). Marked `pub(crate)` of `studio_server::lib.rs` is
/// `attempt_persist_auto_unlock`; we replicate the public-API surface
/// here using `AppState::with_persist` + a manual boot call.
///
/// Today, the auto-unlock function in `studio_server::lib` is private
/// (it's an internal `serve()` helper). For the integration test we
/// either:
///   (a) make it `pub` from `lib.rs` (small but legitimate increase
///       in public-API surface), or
///   (b) replicate the exact algorithm here.
///
/// Picked (a) per ADR-0009 §"Done means" item 2 — the test must
/// exercise the SAME path the caller walks (F1.5 deep-source-read).
/// `studio_server::auto_unlock_on_boot(&state)` is the lift; see
/// `lib.rs` for the implementation.
async fn run_boot_unlock(state: &AppState) {
    studio_server::auto_unlock_on_boot(state).await;
}

/// POST /api/login with the given credentials. Returns the response
/// status + body. Mirrors the `secret_roundtrip.rs::do_login` helper
/// so the test-corpus surface is uniform.
async fn do_login(
    app: &axum::Router,
    endpoint: &str,
    api_key: &str,
    model: &str,
    passphrase: &str,
) -> (StatusCode, Value) {
    let body = json!({
        "endpoint": endpoint,
        "api_key": api_key,
        "model": model,
        "passphrase": passphrase,
    });
    let resp = oneshot_post_json(app, "/api/login", &body).await;
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let json = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// POST /api/logout (with optional ?purge=true). Returns status + body.
async fn do_logout(app: &axum::Router, purge: bool) -> (StatusCode, Value) {
    let uri = if purge {
        "/api/logout?purge=true"
    } else {
        "/api/logout"
    };
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::empty())
        .expect("build request");
    let resp = app.clone().oneshot(req).await.expect("oneshot");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let json = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// GET /api/session/status. Returns `authenticated: bool`.
async fn get_session_status(app: &axum::Router) -> bool {
    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/session/status")
        .body(Body::empty())
        .expect("build request");
    let resp = app.clone().oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status(), StatusCode::OK, "session/status must be 200");
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let value: Value = serde_json::from_slice(&bytes).expect("parse");
    value["authenticated"]
        .as_bool()
        .expect("authenticated field")
}

// ─── Test 1: file-persist path survives restart ────────────────────────────

/// POST /api/login with `--persist-session=file` + tempdir file path →
/// the login handler mirrors the passphrase into the file via
/// `state.persist.save()`. Then drop the AppState, build a NEW
/// AppState pointing at the SAME store + persist file, run the boot
/// auto-unlock, and assert the new AppState's session_key is Some.
///
/// This is the **primary regression gate** for M8 — it proves the
/// dogfooder restart friction is gone.
///
/// Aligns with ADR-0009 §"Done means" item 2 sub-bullet 1.
#[tokio::test]
async fn file_persist_path_survives_restart() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let persist_path = tmp.path().join("passphrase-survives");

    // --- First boot: persist file does not yet exist ---
    let persist1: Arc<dyn PersistStore + Send + Sync> =
        Arc::new(FileStore::new(persist_path.clone()));
    let (state1, app1) = boot_with_persist(root.clone(), persist1.clone()).await;

    // Before login: no session, no persist file.
    assert!(
        !get_session_status(&app1).await,
        "before login: must be unauthenticated"
    );
    assert!(
        !persist_path.exists(),
        "before login: persist file must not exist"
    );

    // POST /api/login — login handler should mirror passphrase to the file.
    let passphrase = "m8-survives-restart-test!";
    let (status, body) = do_login(
        &app1,
        "https://api.anthropic.com",
        "sk-m8-survives",
        "claude-opus-4-7",
        passphrase,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "login must succeed; body={body}");

    // After login: in-memory session_key is Some + persist file is written.
    assert!(
        get_session_status(&app1).await,
        "after login: must be authenticated"
    );
    assert!(
        persist_path.exists(),
        "after login: persist file MUST be written (M8 mirror)"
    );

    // Read the file back manually and confirm the passphrase is the
    // exact bytes we POSTed. This is the deep-source-read check —
    // catches a "saved something but wrong" bug a same-instance load
    // wouldn't catch.
    let file_contents = std::fs::read_to_string(&persist_path).expect("read persist file");
    assert_eq!(
        file_contents, passphrase,
        "persist file MUST contain the exact passphrase"
    );

    // Verify mode 0o600 (Unix only — Windows skips per ADR-0009).
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mode = std::fs::metadata(&persist_path).expect("metadata").mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "persist file MUST be 0600 (got {mode:o}) — ADR-0009 §Wire detail file path"
        );
    }

    // --- Simulate process restart: drop state1, build a fresh state2 ---
    drop(state1);
    drop(app1);

    let persist2: Arc<dyn PersistStore + Send + Sync> = Arc::new(FileStore::new(persist_path));
    let (state2, app2) = boot_with_persist(root, persist2).await;

    // Pre-boot-unlock: fresh AppState starts with session_key=None.
    assert!(
        !get_session_status(&app2).await,
        "fresh AppState pre-boot-unlock: must be unauthenticated"
    );

    // Run the boot auto-unlock — this is what `serve()` does at startup.
    run_boot_unlock(&state2).await;

    // Post-boot-unlock: session_key MUST be Some, status MUST be true.
    assert!(
        get_session_status(&app2).await,
        "post-boot-unlock: M8 must restore session_key from persist file \
         (this is the dogfooder friction fix — failing here means the user \
         still has to re-enter passphrase on every restart)"
    );

    // Verify the restored key is actually usable: read it via the
    // session_key handle directly.
    let key_guard = state2.session_key.read().await;
    assert!(
        key_guard.is_some(),
        "state.session_key MUST be Some after auto-unlock"
    );
}

// ─── Test 2: none-persist drops key on restart ─────────────────────────────

/// POST /api/login with `--persist-session=none` (NullStore) → restart
/// → session_key MUST be None. v0.3.0 baseline regression gate.
///
/// Aligns with ADR-0009 §"Done means" item 2 sub-bullet 3.
#[tokio::test]
async fn none_persist_path_drops_key_on_restart() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();

    // First boot — NullStore (the v0.3.0 baseline).
    let persist1: Arc<dyn PersistStore + Send + Sync> = Arc::new(NullStore);
    let (_state1, app1) = boot_with_persist(root.clone(), persist1).await;

    let passphrase = "none-persist-test!";
    let (status, _) = do_login(
        &app1,
        "https://api.anthropic.com",
        "sk-none-persist",
        "claude-opus-4-7",
        passphrase,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "login must succeed");
    assert!(
        get_session_status(&app1).await,
        "after login: must be authenticated"
    );

    // --- Simulate restart: fresh state, no persist carry-over ---
    drop(app1);

    let persist2: Arc<dyn PersistStore + Send + Sync> = Arc::new(NullStore);
    let (state2, app2) = boot_with_persist(root, persist2).await;

    // Even after running the boot-unlock, NullStore returns Ok(None) so
    // session_key stays None. v0.3.0 baseline preserved.
    run_boot_unlock(&state2).await;
    assert!(
        !get_session_status(&app2).await,
        "after restart with NullStore: must be unauthenticated (v0.3.0 baseline)"
    );
}

// ─── Test 3: logout?purge=true clears file persist ─────────────────────────

/// POST /api/login → assert persist file written → POST /api/logout?
/// purge=true → assert persist file deleted → simulate restart →
/// status=false.
///
/// Aligns with ADR-0009 §"Done means" item 2 sub-bullet 4 + §"On /api/
/// logout".
#[tokio::test]
async fn logout_purge_clears_file_persist() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let persist_path = tmp.path().join("passphrase-purge");

    let persist1: Arc<dyn PersistStore + Send + Sync> =
        Arc::new(FileStore::new(persist_path.clone()));
    let (_state1, app1) = boot_with_persist(root.clone(), persist1.clone()).await;

    // Login mirror writes the file.
    let (status, _) = do_login(
        &app1,
        "https://api.anthropic.com",
        "sk-purge-test",
        "claude-opus-4-7",
        "purge-test-pass-m8!",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(persist_path.exists(), "login must mirror to persist file");

    // Logout with ?purge=true MUST delete the file.
    let (status, body) = do_logout(&app1, true).await;
    assert_eq!(status, StatusCode::OK, "logout?purge=true must succeed");
    assert_eq!(
        body["status"].as_str(),
        Some("ok"),
        "logout body status field"
    );
    assert!(
        !persist_path.exists(),
        "logout?purge=true MUST delete the persist file (ADR-0009 §On /api/logout)"
    );

    // Restart → no auto-unlock possible (file gone) → status=false.
    drop(app1);
    let persist2: Arc<dyn PersistStore + Send + Sync> = Arc::new(FileStore::new(persist_path));
    let (state2, app2) = boot_with_persist(root, persist2).await;
    run_boot_unlock(&state2).await;
    assert!(
        !get_session_status(&app2).await,
        "after purge + restart: must be unauthenticated"
    );
}

// ─── Test 4: regular logout (no purge) preserves file persist ──────────────

/// POST /api/login → POST /api/logout (no purge) → assert persist file
/// still exists → simulate restart → session restored.
///
/// This is the explicit contract from ADR-0009 §"On /api/logout":
/// "By default, logout does NOT clear the keychain/file entry (the
/// user can still re-login by restart without typing the passphrase)."
#[tokio::test]
async fn regular_logout_preserves_file_persist_for_next_restart() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let persist_path = tmp.path().join("passphrase-no-purge");

    let persist1: Arc<dyn PersistStore + Send + Sync> =
        Arc::new(FileStore::new(persist_path.clone()));
    let (_state1, app1) = boot_with_persist(root.clone(), persist1.clone()).await;

    let (status, _) = do_login(
        &app1,
        "https://api.anthropic.com",
        "sk-no-purge",
        "claude-opus-4-7",
        "no-purge-test-pass!",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(persist_path.exists(), "login must mirror");

    // Logout WITHOUT purge — must NOT delete the file.
    let (status, _body) = do_logout(&app1, false).await;
    assert_eq!(status, StatusCode::OK, "plain logout must succeed");
    assert!(
        persist_path.exists(),
        "plain logout MUST preserve persist file (ADR-0009 §On /api/logout)"
    );
    assert!(
        !get_session_status(&app1).await,
        "after plain logout: in-memory session_key MUST be None"
    );

    // Restart → file still there → boot-unlock restores session.
    drop(app1);
    let persist2: Arc<dyn PersistStore + Send + Sync> = Arc::new(FileStore::new(persist_path));
    let (state2, app2) = boot_with_persist(root, persist2).await;
    run_boot_unlock(&state2).await;
    assert!(
        get_session_status(&app2).await,
        "after plain logout + restart: M8 MUST auto-unlock (no purge => persist survives)"
    );
}

// ─── Test 5: wrong persist passphrase invalidates and clears ───────────────

/// Manually write a WRONG passphrase to the persist file → boot →
/// auto-unlock attempts derive+open, fails the `key.open(&blob)`
/// check → auto-clears persist entry + logs warning → status=false.
///
/// This is the "passphrase rotated externally" / "blob corrupted"
/// hazard the M6 seal-salt-mismatch lesson taught us. The boot flow
/// MUST verify the derived key actually opens the blob — not just
/// trust the persist entry.
///
/// Aligns with ADR-0009 §"Done means" item 2 sub-bullet 4 (purge-on-
/// mismatch is implicit in the algorithm; this test is the explicit
/// regression gate).
#[tokio::test]
async fn wrong_persist_passphrase_invalidates_and_clears() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let persist_path = tmp.path().join("passphrase-wrong");

    // --- Step 1: legitimate login seals the blob with CORRECT passphrase ---
    let persist1: Arc<dyn PersistStore + Send + Sync> =
        Arc::new(FileStore::new(persist_path.clone()));
    let (_state1, app1) = boot_with_persist(root.clone(), persist1).await;

    let correct_pass = "correct-passphrase-m8";
    let (status, _) = do_login(
        &app1,
        "https://api.anthropic.com",
        "sk-wrong-persist",
        "claude-opus-4-7",
        correct_pass,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    drop(app1);

    // --- Step 2: tamper with persist file — write a WRONG passphrase ---
    // Mimics the "operator rotated credentials by deleting the blob via
    // sqlite3 but didn't purge the keychain entry, then re-logged in
    // with a NEW passphrase, but the persist entry still holds the OLD
    // one" hazard.
    //
    // Or equivalently: blob is intact (sealed with `correct_pass`) but
    // persist file holds a different passphrase. Boot-unlock derives a
    // key from the wrong passphrase; `key.open(&blob)` fails.
    let wrong_pass = "WRONG-passphrase-stale";
    std::fs::write(&persist_path, wrong_pass).expect("tamper write");
    // Restore 0600 — `fs::write` may set 0644 by default.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&persist_path, perms).expect("chmod");
    }
    assert!(
        persist_path.exists(),
        "persist file still exists post-tamper"
    );

    // --- Step 3: fresh boot — must NOT auto-unlock + must CLEAR persist ---
    let persist2: Arc<dyn PersistStore + Send + Sync> =
        Arc::new(FileStore::new(persist_path.clone()));
    let (state2, app2) = boot_with_persist(root, persist2).await;
    run_boot_unlock(&state2).await;

    // Boot-unlock saw the wrong passphrase → derive succeeded → open()
    // failed → auto-clear fired. Status is unauthenticated and the
    // persist file is GONE.
    assert!(
        !get_session_status(&app2).await,
        "wrong persist passphrase: must NOT auto-unlock"
    );
    assert!(
        !persist_path.exists(),
        "wrong persist passphrase: boot-unlock MUST clear the stale entry \
         to avoid the orphaned-passphrase-drift hazard"
    );
}

// ─── Test 6: orphaned persist (no blob) auto-clears ────────────────────────

/// Persist file has a passphrase but session_kv has no blob (operator
/// deleted the blob via sqlite3 but forgot to purge persist). Boot →
/// boot-unlock detects the orphan + auto-clears persist + status=false.
///
/// Same family as test 5 (stale persist hazard) but distinguishes the
/// "blob missing" path from the "blob present but mismatched" path —
/// they're handled in different branches of the boot flow.
#[tokio::test]
async fn orphaned_persist_with_no_blob_auto_clears() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let persist_path = tmp.path().join("orphaned-passphrase");

    // Write a passphrase to the persist file but NEVER login → no
    // session_kv blob exists.
    std::fs::write(&persist_path, "orphan-pass-m8").expect("write orphan");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&persist_path, perms).expect("chmod");
    }
    assert!(persist_path.exists());

    // Boot — boot-unlock sees passphrase but no blob → orphan path.
    let persist: Arc<dyn PersistStore + Send + Sync> =
        Arc::new(FileStore::new(persist_path.clone()));
    let (state, app) = boot_with_persist(root, persist).await;
    run_boot_unlock(&state).await;

    assert!(
        !get_session_status(&app).await,
        "orphaned persist (no blob): must not auto-unlock"
    );
    assert!(
        !persist_path.exists(),
        "orphaned persist: boot-unlock MUST clear the orphan to avoid stale-credential drift"
    );
}

// ─── Test 7: logout purge surfaces backend clear failure ───────────────────

struct FailingClearStore;

impl PersistStore for FailingClearStore {
    fn save(&self, _passphrase: &str) -> Result<(), PersistError> {
        Ok(())
    }

    fn load(&self) -> Result<Option<zeroize::Zeroizing<String>>, PersistError> {
        Ok(None)
    }

    fn clear(&self) -> Result<(), PersistError> {
        Err(PersistError::File {
            path: std::path::PathBuf::from("/tmp/cobrust-studio-test-failing-clear"),
            source: io::Error::other("clear failed for test"),
        })
    }
}

#[tokio::test]
async fn logout_purge_failure_returns_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let persist: Arc<dyn PersistStore + Send + Sync> = Arc::new(FailingClearStore);
    let (_state, app) = boot_with_persist(root, persist).await;

    let (status, body) = do_logout(&app, true).await;

    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "purge=true must not return ok when persist.clear fails"
    );
    assert_eq!(
        body["code"].as_str(),
        Some("persist_purge_failed"),
        "purge failure must be machine-detectable"
    );
}

// ─── Test 8: keychain path survives restart (#[ignore] for CI) ─────────────

/// Same as `file_persist_path_survives_restart` but uses the OS
/// keychain backend. `#[ignore]`'d by default because CI runners may
/// not have a usable platform keychain (macOS Keychain prompts a GUI;
/// Linux secret-service needs D-Bus + gnome-keyring; Windows usually
/// works but adds platform-specific setup).
///
/// Run locally with:
///   cargo test -p studio-server --test persistent_session keychain_path_survives_restart -- --ignored --nocapture
///
/// ADR-0009 §"Phase 2 caveats" + the dispatch prompt §I document the
/// CI fixture pattern: `#[ignore]` is the discipline boundary; the
/// file-backend tests prove the boot-flow algorithm, keychain is dev-
/// laptop convenience.
#[ignore = "platform-specific keychain access; run locally with --ignored"]
#[tokio::test]
async fn keychain_path_survives_restart() {
    // Constants in the keychain are namespaced (cobrust-studio /
    // session-passphrase) so we can't sandbox — we MUST clean up
    // before + after to avoid colliding with the operator's real
    // session.
    let cleanup = || {
        if let Ok(s) = KeychainStore::new() {
            let _ = s.clear();
        }
    };
    cleanup();

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();

    // First boot — keychain backend.
    let persist1: Arc<dyn PersistStore + Send + Sync> =
        Arc::new(KeychainStore::new().expect("keychain handle"));
    let (_state1, app1) = boot_with_persist(root.clone(), persist1).await;

    let passphrase = "m8-keychain-survives!";
    let (status, _) = do_login(
        &app1,
        "https://api.anthropic.com",
        "sk-keychain-survives",
        "claude-opus-4-7",
        passphrase,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "login must succeed");
    assert!(get_session_status(&app1).await);

    // Restart — fresh AppState + same store + a new KeychainStore handle.
    drop(app1);

    let persist2: Arc<dyn PersistStore + Send + Sync> =
        Arc::new(KeychainStore::new().expect("keychain handle 2"));
    let (state2, app2) = boot_with_persist(root, persist2).await;
    run_boot_unlock(&state2).await;

    assert!(
        get_session_status(&app2).await,
        "after restart with keychain: must auto-unlock (the dogfooder fix)"
    );

    // Clean up so re-runs don't accumulate keychain entries.
    cleanup();
}
