//! `studio-server` — Axum HTTP layer for Cobrust Studio.
//!
//! Wave A3 lands the cross-crate skeleton:
//!
//! - [`AppState`] holds a [`studio_store::Store`] handle + an optional
//!   [`studio_router::Router`] (per ADR-0006 §"Addendum 2026-05-11" the
//!   real router needs a `RouterConfig` + ≥1 provider, so A3 leaves it
//!   `None` and Wave A4 / A5 wires the construction).
//! - [`build_router`] returns the configured Axum [`axum::Router`] with
//!   `/api/health` + `/api/version` mounted, tracing + CORS middleware
//!   layered, and a JSON 404 fallback.
//! - [`serve`] binds the listener and runs `axum::serve` until the
//!   process is signalled (graceful drain deferred to Wave A6+).
//!
//! Wave A4 extends with the real route surface listed in
//! `docs/agent/modules/studio-server.md` §"Public surface (M1 target)":
//! - `POST /api/auth/set-endpoint`
//! - `GET /api/project/current`
//! - `GET|POST /api/adr`, `GET /api/adr/:id`
//! - `GET|POST /api/finding`
//! - `POST /api/dispatch` (SSE, gated on `AppState.router.is_some()`)
//! - `GET /api/ledger/recent`
//! - `GET /api/events` (SSE state-change channel)

pub mod app;
pub mod cli;
pub mod embed;
pub mod error;
pub mod persist;
pub mod router_init;
pub mod routes;
pub mod secret;
pub mod sse;
pub mod state;
pub mod synthetic;

use std::net::SocketAddr;

use futures::StreamExt;
use studio_store::{AdrChangeEvent, FindingChangeEvent, Store};
use tokio::net::TcpListener;

pub use crate::app::build_router;
pub use crate::cli::{Cli, Command, ServeArgs};
pub use crate::error::RouteError;
pub use crate::routes::{HealthResponse, VersionResponse};
pub use crate::sse::{EventEnvelope, EventHub, SSE_BUFFER_CAP};
pub use crate::state::{AppState, DispatchContext};
pub use crate::synthetic::SyntheticProvider;

/// Crate version exposed via the `/api/version` route.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Server-level error. Wraps the surfaces the binary entrypoint can hit
/// during startup; per-route errors are not modelled here (routes
/// return [`axum::response::Response`] directly).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServerError {
    /// [`studio_store::Store::open`] failed during startup.
    #[error("store: {0}")]
    Store(#[from] studio_store::StoreError),
    /// `bind` / `accept` / shutdown signal handling failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// M8 (ADR-0009) — `--persist-session=` flag invariant violation
    /// caught at boot. Today fired only for `mode=file` without
    /// `--persist-session-file`. The CLI is the canonical first gate;
    /// this surface is a defence-in-depth check that catches the same
    /// invariant if a downstream caller constructs `ServeArgs`
    /// manually (e.g. an integration test, a future config-file
    /// front-end).
    #[error("persist config: {0}")]
    Persist(#[from] persist::PersistError),
}

/// Run the server end-to-end: open the store, build the app, bind the
/// listener, serve until shutdown signal.
///
/// `host` and `port` mirror [`ServeArgs`]. Returns once the listener
/// stops accepting connections (currently `axum::serve` runs until the
/// process is signalled — graceful Ctrl-C handling lands with the SSE
/// fan-out work in Wave A6+).
///
/// ## M6 `--dev-api-key` injection
///
/// When `args.dev_api_key` is `Some(key)`, the server constructs a
/// synthetic [`crate::secret::SessionKey`] + [`crate::secret::EndpointSecret`]
/// at boot and writes them to `AppState.session_key` and `session_kv`,
/// bypassing the `/login` UI. This allows Playwright fixtures, CI tests,
/// and headless scripts to authenticate without a browser interaction.
///
/// # Errors
/// Bubbles up [`ServerError`] from store open, bind, or serve loop.
pub async fn serve(args: &ServeArgs) -> Result<(), ServerError> {
    let project_root = args.project.clone();
    let store = Store::open(project_root.clone()).await?;
    // Wave A5: try to construct the router from `<project_root>/studio.toml`.
    // Soft-fail to `None` on any error (missing config, malformed TOML, no
    // credentials, build failure). See ADR-0006 §"Addendum 2026-05-11" F-01
    // for the binding contract, and `crate::router_init` for the resolution
    // order. The `None` path keeps Wave-A4 503 behavior intact.
    let router = router_init::try_build_router_from_project(&project_root, &store).await?;

    // M8 (ADR-0009): build the persistent-session backend before constructing
    // AppState so the boot-time auto-unlock path has the same Arc the login
    // handler will later mirror into. `build_store` hard-errors if
    // `mode=file` without `--persist-session-file` — boot fails fast so the
    // operator sees a clear "missing file path" message instead of silently
    // dropping the persistence option.
    let persist_arc: std::sync::Arc<dyn persist::PersistStore + Send + Sync> =
        persist::build_store(args.persist_session, args.persist_session_file.clone())?.into();

    let mut state = AppState::with_persist(store, router, project_root.clone(), persist_arc);

    // M6: Apply --debug-session flag.
    state.debug_session = args.debug_session;

    // M8 (ADR-0009): auto-unlock — if a persist backend has a passphrase,
    // re-derive the SessionKey now so the next request doesn't need to
    // visit /login. The path is best-effort: failures here (keychain
    // denied, salt missing, derive error, open() mismatch) log a warning
    // and fall through to the v0.3.0 baseline (user re-enters passphrase
    // via /login).
    //
    // Deep-source-read discipline: the re-derive needs the salt from
    // `session_kv.ciphertext[..16]` — same as the wrong-passphrase
    // guard in login.rs. Verify the derived key actually opens the
    // blob before stashing it; this catches the "passphrase rotated
    // externally" hazard (operator deleted the blob via sqlite3 + re-
    // logged in with a new passphrase, but the keychain still holds
    // the OLD passphrase). M6 seal-salt-mismatch lesson lives here.
    auto_unlock_on_boot(&state).await;

    // M6: Apply --dev-api-key escape hatch (ADR-0007 §"Env-var path retention").
    // M7 (ADR-0008): Use --dev-provider-kind to select the provider kind for
    // the boot-time injection (defaults to Anthropic for v0.2.x back-compat).
    if let Some(ref dev_key) = args.dev_api_key {
        use rand_core::{OsRng, RngCore};
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);

        match secret::SessionKey::derive("dev-api-key-synthetic-passphrase", &salt) {
            Ok(session_key) => {
                let secret = secret::EndpointSecret {
                    endpoint: args.dev_endpoint.clone(),
                    api_key: dev_key.clone(),
                    model: args.dev_model.clone(),
                    provider_kind: args.dev_provider_kind,
                };
                match session_key.seal(&secret) {
                    Ok(ciphertext) => {
                        let blob = studio_store::session::EncryptedBlob {
                            ciphertext,
                            nonce: Vec::new(),
                            scheme: secret::SCHEME.to_string(),
                        };
                        if let Err(e) = state.store.session().set_endpoint(blob).await {
                            tracing::warn!(error = %e, "--dev-api-key: failed to persist blob; boot continues");
                        }
                        let mut guard = state.session_key.write().await;
                        *guard = Some(session_key);
                        drop(guard);
                        tracing::info!(
                            endpoint = %args.dev_endpoint,
                            model = %args.dev_model,
                            provider_kind = ?args.dev_provider_kind,
                            "--dev-api-key: synthetic session key injected at boot",
                        );
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "--dev-api-key: seal failed; boot continues unauthenticated");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "--dev-api-key: key derivation failed; boot continues unauthenticated");
            }
        }
    }

    // Wave A4 (A5 reconcile): the watcher → EventHub bridge is now spawned
    // inside [`build_router`] so test harnesses that boot via
    // `build_router(state)` directly (`tests/common/mod.rs::fresh_app`) also
    // get a live bridge without having to duplicate the wiring. The
    // previous explicit call here was redundant once `build_router` took
    // ownership of the spawn.

    let addr: SocketAddr =
        format!("{}:{}", args.host, args.port)
            .parse()
            .map_err(|e: std::net::AddrParseError| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
            })?;
    let listener = TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    tracing::info!(
        %bound,
        project = %project_root.display(),
        "cobrust-studio serving on http://{bound}",
    );
    println!(
        "cobrust-studio serving on http://{bound} project={}",
        project_root.display(),
    );

    let app = build_router(state);
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

/// M8 (ADR-0009) auto-unlock — attempt to restore the in-memory
/// `SessionKey` from the configured persist backend on boot.
///
/// Called by [`serve`] immediately after [`AppState`] is constructed
/// and before `axum::serve` starts accepting connections. Exposed
/// `pub` so integration tests in `tests/persistent_session.rs` can
/// drive the SAME function the binary does — F1.5 deep-source-read
/// discipline (test the path the caller actually walks).
///
/// The flow walks the same algorithm `routes/login.rs` uses on the
/// wrong-passphrase guard (read blob → extract salt → derive →
/// open):
///
/// 1. `persist.load()` → passphrase (or `Ok(None)` → return; user
///    visits /login normally).
/// 2. Read the `session_kv` blob — needed for the salt at
///    `ciphertext[..16]`. If the blob is missing the persist entry
///    is orphaned (operator deleted the blob via sqlite3 to rotate
///    credentials), so clear the persist entry + log a warn.
/// 3. `SessionKey::derive(passphrase, salt)` → candidate key.
/// 4. `key.open(&blob.ciphertext)` to VERIFY — must succeed.
///    Failure means the persist passphrase doesn't match the blob
///    (passphrase rotated externally without clearing persist;
///    blob corrupted; etc.). On failure, auto-clear the persist
///    entry to fail-loud on the next `/login` attempt.
/// 5. Stash the verified key into `state.session_key`. Subsequent
///    `/api/dispatch` calls use the in-memory key as if `/api/login`
///    had just run.
///
/// Every error path collapses to "auto-unlock did not happen; user
/// must /login" so a misconfigured persist backend never blocks the
/// server from booting.
///
/// Calling this against a [`AppState`] whose `persist` is a
/// [`persist::NullStore`] is safe — `NullStore::load()` returns
/// `Ok(None)` so the function returns early at step 1. No need for
/// the caller to gate the call on the backend selector.
pub async fn auto_unlock_on_boot(state: &AppState) {
    // Step 1 — load passphrase from backend.
    let passphrase = match state.persist.load() {
        Ok(Some(p)) => p,
        Ok(None) => {
            tracing::debug!("M8 auto-unlock: persist backend empty; user will /login normally");
            return;
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "M8 auto-unlock: persist backend load failed; user must /login"
            );
            return;
        }
    };

    // Step 2 — read the session_kv blob (the salt source).
    let blob = match state.store.session().get_endpoint().await {
        Ok(Some(b)) => b,
        Ok(None) => {
            // Persist has a passphrase but there's no blob — orphaned
            // entry (operator likely deleted the row via sqlite3 to
            // rotate credentials but forgot to purge persist). Auto-
            // clear so the next /login starts clean.
            tracing::warn!(
                "M8 auto-unlock: persist has a passphrase but session_kv is empty — \
                 orphaned persist entry (passphrase rotated externally? blob deleted?); \
                 clearing persist to avoid stale-credential drift"
            );
            if let Err(e) = state.persist.clear() {
                tracing::warn!(error = %e, "M8 auto-unlock: persist clear failed");
            }
            return;
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "M8 auto-unlock: session_kv read failed; user must /login"
            );
            return;
        }
    };

    // Step 3 — extract salt; require the M6 scheme + ≥16-byte ciphertext.
    if blob.scheme != secret::SCHEME {
        tracing::warn!(
            blob_scheme = %blob.scheme,
            expected_scheme = secret::SCHEME,
            "M8 auto-unlock: blob has unexpected scheme; user must /login"
        );
        return;
    }
    if blob.ciphertext.len() < 16 {
        tracing::warn!(
            blob_len = blob.ciphertext.len(),
            "M8 auto-unlock: blob too short to hold salt; user must /login"
        );
        return;
    }
    let mut salt = [0u8; 16];
    salt.copy_from_slice(&blob.ciphertext[..16]);

    // Step 4 — derive the key.
    let key = match secret::SessionKey::derive(&passphrase, &salt) {
        Ok(k) => k,
        Err(e) => {
            tracing::error!(
                error = %e,
                "M8 auto-unlock: SessionKey::derive failed; user must /login"
            );
            return;
        }
    };

    // Step 5 — VERIFY by opening the blob (catches passphrase mismatch).
    //
    // This is the M6 seal-salt-mismatch lesson applied: the round-trip
    // path is `load passphrase → read blob → derive → OPEN blob`; the
    // open step is what catches a stale persist entry (passphrase
    // doesn't match the salt that sealed the blob).
    if let Err(e) = key.open(&blob.ciphertext) {
        tracing::warn!(
            error = %e,
            "M8 auto-unlock: derived key failed to open blob — \
             passphrase rotated externally? blob corrupted? \
             clearing persist to avoid stale-credential drift; user must /login"
        );
        if let Err(clear_err) = state.persist.clear() {
            tracing::warn!(
                error = %clear_err,
                "M8 auto-unlock: persist clear failed after open() mismatch"
            );
        }
        return;
    }

    // Step 6 — stash the verified key.
    {
        let mut guard = state.session_key.write().await;
        *guard = Some(key);
    }
    tracing::info!("M8 auto-unlock: session restored from persist backend; no /login needed");
}

/// Spawn two background tasks that drain
/// [`studio_store::adr::AdrHandle::watch`] and
/// [`studio_store::finding::FindingHandle::watch`] into the SSE event
/// hub. Tasks live until the underlying `notify` watcher closes (which
/// happens when the `Store` is dropped at process shutdown).
///
/// Public so test harnesses can pre-arm the bridge before issuing
/// filesystem events.
pub fn spawn_watcher_bridge(state: &AppState) {
    let adr_stream = state.store().adr().watch();
    let events_adr = state.events().clone();
    tokio::spawn(async move {
        let mut s = std::pin::pin!(adr_stream);
        while let Some(evt) = s.next().await {
            let envelope = match evt {
                AdrChangeEvent::Added(p) => sse::EventEnvelope::AdrAdded {
                    path: p.display().to_string(),
                },
                AdrChangeEvent::Modified(p) => sse::EventEnvelope::AdrModified {
                    path: p.display().to_string(),
                },
                AdrChangeEvent::Removed(p) => sse::EventEnvelope::AdrRemoved {
                    path: p.display().to_string(),
                },
            };
            events_adr.publish(envelope);
        }
        tracing::debug!("adr watcher stream closed; bridge task exiting");
    });

    let finding_stream = state.store().finding().watch();
    let events_finding = state.events().clone();
    tokio::spawn(async move {
        let mut s = std::pin::pin!(finding_stream);
        while let Some(evt) = s.next().await {
            let envelope = match evt {
                FindingChangeEvent::Added(p) => sse::EventEnvelope::FindingAdded {
                    path: p.display().to_string(),
                },
                FindingChangeEvent::Modified(p) => sse::EventEnvelope::FindingModified {
                    path: p.display().to_string(),
                },
                FindingChangeEvent::Removed(p) => sse::EventEnvelope::FindingRemoved {
                    path: p.display().to_string(),
                },
            };
            events_finding.publish(envelope);
        }
        tracing::debug!("finding watcher stream closed; bridge task exiting");
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn version_is_pkg_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn build_router_smokes_with_real_state() {
        // Cross-crate integration probe: prove the server can open a
        // Store and wrap it in an AppState the Axum app accepts.
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open(tmp.path()).await.expect("Store::open");
        let state = AppState::new(store, None, tmp.path().to_path_buf());
        let _router = build_router(state);
        // The fact we got here means the type plumbing compiles + runs;
        // P7 TEST's hyper-level integration tests assert the response
        // bodies.
    }

    #[tokio::test]
    async fn uptime_is_monotonic_nondecreasing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open(tmp.path()).await.expect("Store::open");
        let state = AppState::new(store, None, tmp.path().to_path_buf());
        let a = state.uptime_seconds();
        // No sleep — just two reads; the assertion is `>=`, not `>`.
        let b = state.uptime_seconds();
        assert!(b >= a, "uptime went backwards: {a} -> {b}");
    }
}
