//! Router construction at server boot.
//!
//! Wave A5 lands the studio.toml → `studio_router::Router` path described
//! in ADR-0006 §"Addendum 2026-05-11" §F-01:
//!
//! ```rust,ignore
//! let cfg = RouterConfig::from_toml_str(&toml)?;
//! let provider: Arc<dyn LlmProvider> =
//!     Arc::new(AnthropicProvider::new(/* … */)?);
//! let router = RouterBuilder::new()
//!     .register_provider("anthropic_official", provider)
//!     .build(&cfg)
//!     .await?;
//! ```
//!
//! The function is fallible-soft: any failure (missing config, malformed
//! TOML, unregistered provider, AEAD round-trip not yet implemented)
//! resolves to `Ok(None)` after a `tracing::warn!`. The contract for the
//! dispatch route is unchanged — `AppState.router = None` keeps the
//! Wave-A4 `503 router_not_configured` behavior intact. The constitution
//! says "default to proceed" and the M2 frontend already renders a "router
//! not configured" banner against the 503, so a soft failure here is the
//! right UX.
//!
//! ## Credential model (A5 stub)
//!
//! Real per-provider credential decryption lands at M2 (the auth flow
//! that round-trips an AEAD-encrypted blob through the SvelteKit login
//! page is post-M1). For Wave A5, the credential resolution order is:
//!
//! 1. `api_key_env` (from each `[providers.X]` block in `studio.toml`)
//!    is read from process env. If set + non-empty → use it.
//! 2. Otherwise, fall back to the project-scoped `EncryptedBlob` stored
//!    via `Store::session().get_endpoint()`. **A5 STUB**: the blob is
//!    treated as opaque raw bytes (scheme = `"raw"`) and the ciphertext
//!    is used as the API-key string for every non-synthetic provider.
//!    Real AEAD decryption (`scheme = "aes-gcm-256/argon2id"`) lands at
//!    M2 — at which point this fallback grows into a proper decrypt
//!    helper. CLAUDE.md §3.1 still applies: API keys never get echoed
//!    into source or logs.
//! 3. If neither source yields a key, the non-synthetic provider is
//!    skipped (logged as a warn). `ProviderKind::Synthetic` providers
//!    never need a credential — they're always registered.
//!
//! ## Determinism
//!
//! `BTreeMap` iteration order means provider-registration order is
//! lexicographic by config-key — important so two boots of the same
//! `studio.toml` produce identical `Router::Debug` output (relevant for
//! the F20 audit trail).

use std::path::Path;
use std::sync::Arc;

use studio_router::{
    AnthropicProvider, LlmProvider, OpenAiProvider, ProviderConfig, ProviderKind, Router,
    RouterBuilder, RouterConfig,
};
use studio_store::Store;

use crate::synthetic::SyntheticProvider;

/// File name searched at the project root for the router config.
///
/// Two-tiered lookup: `studio.toml` first (matches the v0.1.x default
/// shipping spec), then `cobrust-studio.toml` for users who prefix the
/// file with the binary name. Both shapes are accepted by `RouterConfig`.
pub const PRIMARY_CONFIG_FILE: &str = "studio.toml";
/// Secondary lookup name; same parse shape as [`PRIMARY_CONFIG_FILE`].
pub const ALTERNATE_CONFIG_FILE: &str = "cobrust-studio.toml";

/// Errors raised internally; never bubbled to callers — they're logged
/// and the router falls through to `None`.
///
/// `Parse` carries the `toml::de` message as a `String` rather than the
/// concrete `toml::de::Error` to keep studio-server's direct dep list
/// minimal (toml is already a transitive dep via studio-router; we
/// don't need a direct `[dependencies] toml` entry just for a type
/// signature).
#[derive(Debug, thiserror::Error)]
enum InitError {
    #[error("read config: {0}")]
    Read(#[from] std::io::Error),
    #[error("parse config: {0}")]
    Parse(String),
    #[error("router build: {0}")]
    Build(String),
    #[error("store: {0}")]
    Store(#[from] studio_store::StoreError),
}

/// Attempt to construct a [`Router`] from a project-root `studio.toml`.
///
/// Returns `Ok(None)` when:
///
/// - Neither [`PRIMARY_CONFIG_FILE`] nor [`ALTERNATE_CONFIG_FILE`] exists
///   under `project_root`.
/// - The config parses but no provider survives credential resolution.
/// - `RouterBuilder::build` fails (e.g. preferred list references an
///   unregistered provider).
///
/// **Soft-fail by design** (see module-level doc): any failure path logs
/// a structured `tracing::warn!` so operators can diagnose without the
/// server bailing on boot. The dispatch route falls back to its A4
/// `503 router_not_configured` shape, which the M2 frontend already
/// renders as a friendly banner.
///
/// # Errors
/// Never returns `Err` — failures collapse to `Ok(None)`. The
/// `Result` type stays for forward-compat (M2 may want a hard-fail
/// option behind a flag).
#[allow(clippy::unused_async)]
pub async fn try_build_router_from_project(
    project_root: &Path,
    store: &Store,
) -> std::io::Result<Option<Arc<Router>>> {
    match build_inner(project_root, store).await {
        Ok(Some(r)) => Ok(Some(Arc::new(r))),
        Ok(None) => Ok(None),
        Err(e) => {
            tracing::warn!(
                error = %e,
                project = %project_root.display(),
                "router boot failed; /api/dispatch will return 503",
            );
            Ok(None)
        }
    }
}

async fn build_inner(project_root: &Path, store: &Store) -> Result<Option<Router>, InitError> {
    let Some(toml_text) = read_config(project_root).await? else {
        tracing::info!(
            project = %project_root.display(),
            primary = PRIMARY_CONFIG_FILE,
            alternate = ALTERNATE_CONFIG_FILE,
            "no router config found; /api/dispatch will return 503 until M2 auth lands",
        );
        return Ok(None);
    };

    let cfg =
        RouterConfig::from_toml_str(&toml_text).map_err(|e| InitError::Parse(e.to_string()))?;

    // Sarah v3 audit #2: `api_key_env` in studio.toml is a second
    // credential-resolution path that bypasses the /login flow. With M6
    // AEAD round-trip shipped, the canonical credential path is /login →
    // session_kv → in-memory SessionKey. Keep `api_key_env` working for
    // backward compat + CI/headless flows but log a startup warning when
    // any non-empty value is detected so operators have a clear migration
    // signal. v0.3.x will introduce a strict mode that errors instead.
    for (name, pcfg) in &cfg.providers {
        if !pcfg.api_key_env.is_empty() {
            tracing::warn!(
                provider = name,
                api_key_env = %pcfg.api_key_env,
                "studio.toml `api_key_env` is deprecated in v0.2.x; v0.3.x will require /login or --dev-api-key. \
                 See docs/human/en/secret-storage.md for migration.",
            );
        }
    }

    // Fetch the encrypted-blob fallback once — it's the same value for
    // every non-synthetic provider (A5 stub model).
    let blob_bytes: Option<Vec<u8>> = store.session().get_endpoint().await?.map(|b| b.ciphertext);

    let mut builder = RouterBuilder::new();
    let mut registered_count = 0_usize;
    for (name, pcfg) in &cfg.providers {
        match register_one(&mut builder, name, pcfg, blob_bytes.as_deref()) {
            Some(b) => {
                builder = b;
                registered_count += 1;
            }
            None => {
                tracing::warn!(
                    provider = name,
                    kind = ?pcfg.kind,
                    api_key_env = %pcfg.api_key_env,
                    "skipping provider: no credential available (env-var unset; no session blob)",
                );
            }
        }
    }

    if registered_count == 0 {
        tracing::warn!(
            "studio.toml had providers but none could be constructed (no credentials); router stays None",
        );
        return Ok(None);
    }

    let router = builder
        .build(&cfg)
        .await
        .map_err(|e| InitError::Build(e.to_string()))?;
    tracing::info!(
        registered = registered_count,
        preferred = cfg.router.preferred.len(),
        "router boot OK",
    );
    Ok(Some(router))
}

/// Try to register a single `[providers.<name>]` entry on the builder.
///
/// Returns `Some(builder)` on success and `None` when the provider must
/// be skipped (e.g. no credential available). Synthetic providers never
/// need credentials and always register.
fn register_one(
    builder: &mut RouterBuilder,
    name: &str,
    cfg: &ProviderConfig,
    blob_fallback: Option<&[u8]>,
) -> Option<RouterBuilder> {
    let arc: Arc<dyn LlmProvider> = match cfg.kind {
        ProviderKind::Synthetic => Arc::new(SyntheticProvider::new(name)),
        ProviderKind::Anthropic => {
            let api_key = resolve_api_key(&cfg.api_key_env, blob_fallback)?;
            match AnthropicProvider::new(name.to_string(), cfg.base_url.clone(), api_key) {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    tracing::warn!(provider = name, error = %e, "anthropic provider build failed");
                    return None;
                }
            }
        }
        ProviderKind::Openai => {
            let api_key = resolve_api_key(&cfg.api_key_env, blob_fallback)?;
            match OpenAiProvider::new(name.to_string(), cfg.base_url.clone(), api_key) {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    tracing::warn!(provider = name, error = %e, "openai provider build failed");
                    return None;
                }
            }
        }
    };
    // `RouterBuilder::register_provider` consumes self → return new value
    let taken = std::mem::take(builder);
    Some(taken.register_provider(name.to_string(), arc))
}

/// Resolve an API key from `api_key_env` first, falling back to the
/// raw bytes of the session blob.
///
/// The blob fallback is the **A5 stub** described in module-level docs:
/// treat `ciphertext` as the literal API-key string. M2 swaps this for
/// a real AEAD round-trip.
fn resolve_api_key(api_key_env: &str, blob_fallback: Option<&[u8]>) -> Option<String> {
    if !api_key_env.is_empty()
        && let Ok(v) = std::env::var(api_key_env)
        && !v.is_empty()
    {
        return Some(v);
    }
    let bytes = blob_fallback?;
    if bytes.is_empty() {
        return None;
    }
    // The blob is opaque per CLAUDE.md §3.1 / ADR-0003. For A5 we accept
    // either a UTF-8 string ciphertext (the "raw" stub) or a byte slice
    // — but only the UTF-8 case is useful as an HTTP `Authorization`
    // value, so non-UTF-8 bytes get dropped with a warn.
    if let Ok(s) = std::str::from_utf8(bytes) {
        Some(s.to_string())
    } else {
        tracing::warn!(
            "session blob ciphertext is not UTF-8; non-raw AEAD scheme decryption is post-M1 (M2 auth flow)",
        );
        None
    }
}

async fn read_config(project_root: &Path) -> std::io::Result<Option<String>> {
    for name in [PRIMARY_CONFIG_FILE, ALTERNATE_CONFIG_FILE] {
        let path = project_root.join(name);
        if path.exists() {
            let text = tokio::fs::read_to_string(&path).await?;
            tracing::info!(path = %path.display(), "router config loaded");
            return Ok(Some(text));
        }
    }
    Ok(None)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use studio_store::Store;
    use tempfile::TempDir;

    async fn empty_store(dir: &TempDir) -> Store {
        Store::open(dir.path()).await.expect("Store::open")
    }

    #[tokio::test]
    async fn returns_none_when_no_config_present() {
        let tmp = tempfile::tempdir().unwrap();
        let store = empty_store(&tmp).await;
        let r = try_build_router_from_project(tmp.path(), &store)
            .await
            .unwrap();
        assert!(r.is_none(), "missing studio.toml must yield None");
    }

    #[tokio::test]
    async fn returns_some_with_synthetic_only_config() {
        let tmp = tempfile::tempdir().unwrap();
        let toml_path = tmp.path().join(PRIMARY_CONFIG_FILE);
        let toml = r#"
[router]
strategy = "quality"
preferred = ["synth_a:synthetic-1"]

[providers.synth_a]
kind = "synthetic"
base_url = "https://example.invalid"
api_key_env = ""
models = ["synthetic-1"]
"#;
        tokio::fs::write(&toml_path, toml).await.unwrap();
        let store = empty_store(&tmp).await;
        let r = try_build_router_from_project(tmp.path(), &store)
            .await
            .unwrap();
        assert!(r.is_some(), "synthetic-only config must yield Some");
    }

    #[tokio::test]
    async fn returns_none_when_provider_lacks_credentials() {
        let tmp = tempfile::tempdir().unwrap();
        let toml_path = tmp.path().join(PRIMARY_CONFIG_FILE);
        // anthropic kind with an env-var name that we'll guarantee is empty
        // and no session blob.
        let toml = r#"
[router]
strategy = "quality"
preferred = ["anth:claude-x"]

[providers.anth]
kind = "anthropic"
base_url = "https://api.anthropic.com"
api_key_env = "STUDIO_A5_TEST_DEFINITELY_UNSET_KEY_NAME_XYZ"
models = ["claude-x"]
"#;
        tokio::fs::write(&toml_path, toml).await.unwrap();
        // Defensive: clear the env var if some prior test set it
        unsafe { std::env::remove_var("STUDIO_A5_TEST_DEFINITELY_UNSET_KEY_NAME_XYZ") };
        let store = empty_store(&tmp).await;
        let r = try_build_router_from_project(tmp.path(), &store)
            .await
            .unwrap();
        assert!(
            r.is_none(),
            "anthropic provider with no env-var + no blob must yield None",
        );
    }

    #[tokio::test]
    async fn malformed_toml_resolves_to_none() {
        let tmp = tempfile::tempdir().unwrap();
        let toml_path = tmp.path().join(PRIMARY_CONFIG_FILE);
        tokio::fs::write(&toml_path, "not valid = [ toml")
            .await
            .unwrap();
        let store = empty_store(&tmp).await;
        let r = try_build_router_from_project(tmp.path(), &store)
            .await
            .unwrap();
        assert!(r.is_none(), "malformed TOML must collapse to None");
    }

    #[tokio::test]
    async fn alternate_config_file_is_also_picked_up() {
        let tmp = tempfile::tempdir().unwrap();
        let toml_path = tmp.path().join(ALTERNATE_CONFIG_FILE);
        let toml = r#"
[router]
strategy = "quality"
preferred = ["synth:synth-1"]

[providers.synth]
kind = "synthetic"
base_url = "https://example.invalid"
api_key_env = ""
"#;
        tokio::fs::write(&toml_path, toml).await.unwrap();
        let store = empty_store(&tmp).await;
        let r = try_build_router_from_project(tmp.path(), &store)
            .await
            .unwrap();
        assert!(r.is_some(), "cobrust-studio.toml must be a valid fallback");
    }
}
