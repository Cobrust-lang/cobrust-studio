//! Synthetic [`studio_router::LlmProvider`] implementation.
//!
//! Wave A5 needs a way to exercise the `/api/dispatch` SSE path end-to-end
//! without touching a real LLM (and without dragging an API key into the
//! integration-test corpus). This module ships a deterministic in-process
//! provider that mirrors the upstream `ProviderKind::Synthetic` config
//! variant (see `studio_router::config::ProviderKind`).
//!
//! ## Why this lives in studio-server, not studio-router
//!
//! The Wave A5 task prompt explicitly forbids modifying studio-router
//! source. Upstream Cobrust never lifted a `SyntheticProvider` — the
//! `Synthetic` variant exists in `ProviderKind` for ledger labelling
//! only — so studio-server hosts the impl. If a future ADR migrates
//! studio-router to a crates.io facade, the synthetic provider stays
//! here (it's Studio-test scaffolding, not part of the router contract).
//!
//! ## Determinism
//!
//! Each call to [`SyntheticProvider::complete_stream`] emits exactly:
//!
//! - 4 [`Chunk::Delta`] events with the literal payloads
//!   `"hello"`, `" "`, `"world"`, `"!"` (concatenation: `"hello world!"`).
//! - 1 trailing [`Chunk::Done`] with `prompt_tokens = 3`,
//!   `completion_tokens = 4` (one token per delta).
//!
//! [`SyntheticProvider::complete`] returns the concatenated text
//! (`"hello world!"`) plus the same token usage. The payload is
//! independent of the request — tests rely on this for determinism.
//!
//! ## Public visibility
//!
//! `pub` (not `pub(crate)`) so studio-server integration tests under
//! `tests/` can construct it directly when they want to exercise the
//! dispatch SSE happy path without a real provider. Module-level doc
//! marks it as test-only scaffolding.

use std::pin::Pin;

use futures::stream::{self, Stream};
use studio_router::{
    Chunk, CompletionRequest, CompletionResponse, LlmError, LlmProvider, ProviderKind, TokenUsage,
};

/// Deterministic in-process [`LlmProvider`].
///
/// Useful for tests + the no-credentials dev mode where the user has a
/// `studio.toml` listing `kind = "synthetic"` providers but no API keys
/// in flight.
#[derive(Clone, Debug)]
pub struct SyntheticProvider {
    name: String,
}

impl SyntheticProvider {
    /// Construct with a stable provider key — must match a
    /// `[providers.<name>]` entry in the project's `studio.toml`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// Fixed payload: `"hello world!"` — the concatenation of the four
    /// `Chunk::Delta` frames emitted by [`Self::complete_stream`].
    pub const FIXTURE_TEXT: &'static str = "hello world!";

    /// Fixed usage: 3 prompt tokens (arbitrary stand-in), 4 completion
    /// tokens (one per emitted delta).
    #[must_use]
    pub const fn fixture_usage() -> TokenUsage {
        TokenUsage {
            prompt_tokens: 3,
            completion_tokens: 4,
        }
    }

    fn fixture_deltas() -> [&'static str; 4] {
        ["hello", " ", "world", "!"]
    }
}

#[async_trait::async_trait]
impl LlmProvider for SyntheticProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Synthetic
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Ok(CompletionResponse {
            text: Self::FIXTURE_TEXT.to_string(),
            model: req.model,
            usage: Self::fixture_usage(),
        })
    }

    fn complete_stream(
        &self,
        _req: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<Chunk, LlmError>> + Send>> {
        let mut frames: Vec<Result<Chunk, LlmError>> = Self::fixture_deltas()
            .iter()
            .map(|s| Ok(Chunk::Delta((*s).to_string())))
            .collect();
        frames.push(Ok(Chunk::Done(Self::fixture_usage())));
        Box::pin(stream::iter(frames))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use studio_router::Message;
    use studio_router::Role;
    use studio_router::SamplingParams;

    fn sample_request() -> CompletionRequest {
        CompletionRequest {
            model: "synthetic-1".to_string(),
            messages: vec![Message {
                role: Role::User,
                content: "hi".to_string(),
            }],
            params: SamplingParams::default(),
        }
    }

    #[tokio::test]
    async fn complete_returns_fixture_text() {
        let p = SyntheticProvider::new("synth_test");
        let resp = p.complete(sample_request()).await.unwrap();
        assert_eq!(resp.text, SyntheticProvider::FIXTURE_TEXT);
        assert_eq!(resp.usage, SyntheticProvider::fixture_usage());
        assert_eq!(resp.model, "synthetic-1");
    }

    #[tokio::test]
    async fn complete_stream_emits_four_deltas_then_done() {
        let p = SyntheticProvider::new("synth_test");
        let mut s = p.complete_stream(sample_request());
        let mut deltas: Vec<String> = Vec::new();
        let mut done_seen = false;
        while let Some(item) = s.next().await {
            match item.unwrap() {
                Chunk::Delta(d) => deltas.push(d),
                Chunk::Done(usage) => {
                    assert!(!done_seen, "Done frame must appear exactly once");
                    done_seen = true;
                    assert_eq!(usage, SyntheticProvider::fixture_usage());
                }
            }
        }
        assert!(done_seen, "stream must terminate with a Done frame");
        assert_eq!(deltas.len(), 4);
        assert_eq!(deltas.concat(), SyntheticProvider::FIXTURE_TEXT);
    }

    #[test]
    fn name_round_trips() {
        let p = SyntheticProvider::new("hello");
        assert_eq!(p.name(), "hello");
    }

    #[test]
    fn kind_is_synthetic() {
        let p = SyntheticProvider::new("x");
        assert_eq!(p.kind(), ProviderKind::Synthetic);
    }
}
