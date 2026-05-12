//! `studio.toml` parsing for the LLM Router.
//!
//! Lifted from `cobrust-llm-router` @ `61f2aff` (v0.1.1) per ADR-0005 /
//! ADR-0006. Strips (ADR-0006):
//!
//! - #1 — `Consensus` mode removed (no `Strategy::Consensus`,
//!   no `StrategyName::Consensus`, no `n` parameter).
//! - #3 — Per-task routing tables removed: `RoutingEntry` /
//!   `StrategyName` / `DefaultStrategy` / `routing` map are gone. The
//!   router now has a single global dispatch table.
//! - #5 — Default cache + ledger paths moved from `.cobrust/` to the
//!   Studio namespace (`$XDG_DATA_HOME/cobrust-studio/` or
//!   `$HOME/.cache/cobrust-studio/`).

// Upstream copyright: The Cobrust Project. Licensed under Apache-2.0 OR MIT.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Routing strategy. Strip #1 removed the `Consensus` variant; this enum is
/// now a closed set of three single-provider strategies.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    /// Cheapest configured provider first.
    Cost,
    /// Highest-quality configured provider first (default).
    #[default]
    Quality,
    /// Lowest observed EWMA-latency provider first.
    Latency,
}

/// Provider-API kind.
///
/// `Synthetic` exists for in-process mock providers (deterministic test
/// doubles); it never appears in user-on-disk `studio.toml` because no wire
/// protocol matches it. Recorded in `LedgerEntry::provider_kind` for honest
/// provenance.
///
/// ## M7 addition (ADR-0008)
///
/// `Default` is `Anthropic` so that `#[serde(default)]` on
/// `EndpointSecret::provider_kind` and `LoginRequest::provider_kind` in
/// `studio-server` yields `Anthropic` when the field is absent from the JSON
/// payload — preserving backward compatibility with v0.2.x callers.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[default]
    Anthropic,
    Openai,
    Synthetic,
}

/// `[providers.<name>]` section.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key_env: String,
    #[serde(default)]
    pub models: Vec<String>,
}

/// `[router]` section.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RouterSection {
    /// Default strategy when the dispatch table omits its own.
    #[serde(default)]
    pub strategy: Strategy,
    #[serde(default = "default_cache_dir")]
    pub cache_dir: PathBuf,
    #[serde(default = "default_ledger_path")]
    pub ledger_path: PathBuf,
    /// Ordered list of `provider:model` tags. The router walks them in
    /// strategy-order and falls through on permanent failure. Strip #3
    /// collapses Cobrust's per-task routing into this single global list.
    #[serde(default)]
    pub preferred: Vec<String>,
}

/// Default cache directory. Strip #5 (ADR-0006) — Studio namespace.
///
/// Resolution order:
/// 1. `$XDG_DATA_HOME/cobrust-studio/llm_cache` if `XDG_DATA_HOME` is set
///    and non-empty.
/// 2. `$HOME/.cache/cobrust-studio/llm_cache` otherwise (macOS + Linux).
/// 3. Relative `./.cobrust-studio/llm_cache` if `HOME` is also unset
///    (CI / sandbox edge case).
#[must_use]
pub fn default_cache_dir() -> PathBuf {
    studio_subdir("llm_cache")
}

/// Default ledger path. Strip #5 (ADR-0006) — Studio namespace.
#[must_use]
pub fn default_ledger_path() -> PathBuf {
    studio_subdir("ledger.jsonl")
}

fn studio_subdir(leaf: &str) -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join("cobrust-studio").join(leaf);
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        return PathBuf::from(home)
            .join(".cache")
            .join("cobrust-studio")
            .join(leaf);
    }
    PathBuf::from(".cobrust-studio").join(leaf)
}

impl Default for RouterSection {
    fn default() -> Self {
        Self {
            strategy: Strategy::default(),
            cache_dir: default_cache_dir(),
            ledger_path: default_ledger_path(),
            preferred: Vec::new(),
        }
    }
}

/// Top-level router configuration. Build via [`RouterConfig::from_toml_str`].
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RouterConfig {
    #[serde(default)]
    pub router: RouterSection,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
}

/// Parsed `provider:model` pair.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProviderModel {
    pub provider: String,
    pub model: String,
}

impl ProviderModel {
    /// Parse `"provider:model"`. Returns `None` if the format is wrong.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.splitn(2, ':');
        let provider = parts.next()?.trim();
        let model = parts.next()?.trim();
        if provider.is_empty() || model.is_empty() {
            return None;
        }
        Some(Self {
            provider: provider.to_string(),
            model: model.to_string(),
        })
    }
}

impl RouterConfig {
    /// Parse a `studio.toml` document.
    ///
    /// # Errors
    /// Returns the message produced by `toml::de`.
    pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// Validate cross-field invariants:
    /// 1. Every entry in `router.preferred` parses as `provider:model`.
    /// 2. Every referenced provider is declared in `providers`.
    ///
    /// The model in each pair is *not* required to appear in
    /// `providers.<name>.models` (providers may add new models post-config,
    /// matching upstream behaviour).
    ///
    /// # Errors
    /// Returns a string describing the first violation found.
    pub fn validate(&self) -> Result<(), String> {
        for tag in &self.router.preferred {
            let pm = ProviderModel::parse(tag)
                .ok_or_else(|| format!("router.preferred: malformed provider:model tag {tag:?}"))?;
            if !self.providers.contains_key(&pm.provider) {
                return Err(format!(
                    "router.preferred: provider {:?} referenced by {tag:?} is not declared",
                    pm.provider
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const FULL_TOML: &str = r#"
[router]
strategy = "quality"
preferred = [
    "anthropic_official:claude-opus-4-7",
    "deepseek:deepseek-v3",
]

[providers.anthropic_official]
kind = "anthropic"
base_url = "https://api.anthropic.com"
api_key_env = "ANTHROPIC_API_KEY"
models = ["claude-opus-4-7", "claude-sonnet-4-6"]

[providers.openai_official]
kind = "openai"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
models = ["gpt-5", "gpt-5-mini"]

[providers.deepseek]
kind = "openai"
base_url = "https://api.deepseek.com/v1"
api_key_env = "DEEPSEEK_API_KEY"
models = ["deepseek-v3"]
"#;

    #[test]
    fn parses_full_example_config() {
        let cfg = RouterConfig::from_toml_str(FULL_TOML).expect("must parse");
        assert_eq!(cfg.router.strategy, Strategy::Quality);
        assert_eq!(cfg.providers.len(), 3);
        assert_eq!(cfg.router.preferred.len(), 2);
        assert_eq!(
            cfg.providers["anthropic_official"].kind,
            ProviderKind::Anthropic
        );
        assert_eq!(
            cfg.providers["deepseek"].base_url,
            "https://api.deepseek.com/v1"
        );
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn provider_model_parses_pair() {
        let pm = ProviderModel::parse("anthropic_official:claude-opus-4-7").unwrap();
        assert_eq!(pm.provider, "anthropic_official");
        assert_eq!(pm.model, "claude-opus-4-7");
    }

    #[test]
    fn provider_model_rejects_malformed_input() {
        assert!(ProviderModel::parse("no_colon").is_none());
        assert!(ProviderModel::parse(":model").is_none());
        assert!(ProviderModel::parse("provider:").is_none());
    }

    #[test]
    fn validate_flags_unknown_provider() {
        let toml = r#"
[providers.x]
kind = "openai"
base_url = "http://x"
api_key_env = "X"

[router]
strategy = "quality"
preferred = ["y:m"]
"#;
        let cfg = RouterConfig::from_toml_str(toml).unwrap();
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("not declared"), "{err}");
    }

    #[test]
    fn validate_flags_malformed_tag() {
        let toml = r#"
[providers.x]
kind = "openai"
base_url = "http://x"
api_key_env = "X"

[router]
strategy = "cost"
preferred = ["no-colon-here"]
"#;
        let cfg = RouterConfig::from_toml_str(toml).unwrap();
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("malformed"), "{err}");
    }

    #[test]
    fn defaults_apply_when_router_section_omitted() {
        let toml = r#"
[providers.x]
kind = "openai"
base_url = "http://x"
api_key_env = "X"
"#;
        let cfg = RouterConfig::from_toml_str(toml).unwrap();
        assert_eq!(cfg.router.strategy, Strategy::Quality);
        // The defaults must land under the cobrust-studio namespace.
        let cache_s = cfg.router.cache_dir.to_string_lossy();
        let ledger_s = cfg.router.ledger_path.to_string_lossy();
        assert!(cache_s.contains("cobrust-studio"), "{cache_s}");
        assert!(ledger_s.contains("cobrust-studio"), "{ledger_s}");
        assert!(cache_s.ends_with("llm_cache"));
        assert!(ledger_s.ends_with("ledger.jsonl"));
    }

    #[test]
    fn strategy_serde_is_lowercase() {
        let s = serde_json::to_string(&Strategy::Quality).unwrap();
        assert_eq!(s, "\"quality\"");
        let back: Strategy = serde_json::from_str("\"latency\"").unwrap();
        assert_eq!(back, Strategy::Latency);
    }
}
