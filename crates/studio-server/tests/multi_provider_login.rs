//! M7 multi-provider /login — integration test skeleton (ADR-0008 Phase 1).
//!
//! Per ADR-0008 §"Done means" item 2, six integration scenarios gate M7
//! closure. Each is currently `#[ignore]`-attributed with an
//! `unimplemented!()` body so the file compiles (so the broader workspace
//! `cargo test --workspace --locked` gate stays green during Phase 1) AND
//! the Phase 2 P9 sub-agent has a clear test target to un-ignore +
//! implement.
//!
//! Run only the un-ignored M7 tests after Phase 2 lands with:
//!   cargo test -p studio-server --test multi_provider_login
//!
//! ADR-0008 binds the wire format (additive `provider_kind` field on
//! `LoginRequest` + `EndpointSecret`, defaulting to `Anthropic` for
//! backward compat) + the dispatch-time match arm. Tests assert the
//! deployed implementation honors that pin.

#![allow(clippy::unwrap_used, clippy::expect_used)]

/// POST /api/login with `provider_kind: "anthropic"` → wiremock Anthropic
/// stub at the supplied endpoint → first dispatch round-trips through
/// `AnthropicProvider`. The default-Anthropic path is the back-compat
/// guarantee for v0.2.x callers.
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 1.
#[ignore = "Phase 2 P9 unblocks — ADR-0008 M7 not yet implemented"]
#[tokio::test]
async fn login_anthropic_then_dispatch() {
    unimplemented!(
        "ADR-0008 Phase 2 P9 deliverable: POST /api/login with \
         provider_kind=anthropic → wiremock Anthropic stub → dispatch 200."
    );
}

/// POST /api/login with `provider_kind: "openai"` → wiremock OpenAI
/// `/v1/chat/completions` stub → dispatch round-trips through
/// `OpenAiProvider`. This is the primary M7 regression gate — proves
/// the SvelteKit-form path unblocks OpenAI-compat endpoints (vLLM /
/// DeepSeek / Together / OpenRouter / Groq / local Ollama).
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 2 + closes
/// Sarah v3 audit finding #3 (multi-provider /login).
#[ignore = "Phase 2 P9 unblocks — ADR-0008 M7 not yet implemented"]
#[tokio::test]
async fn login_openai_then_dispatch() {
    unimplemented!(
        "ADR-0008 Phase 2 P9 deliverable: POST /api/login with \
         provider_kind=openai → wiremock OpenAI /v1/chat/completions \
         stub → dispatch 200 via OpenAiProvider."
    );
}

/// POST /api/login with `provider_kind: "synthetic"` → 400
/// `{code: "invalid_provider_kind"}`. The synthetic provider is a
/// CLI/dev-only construct that has no real-world endpoint+key pair;
/// driving it through /login is a category error.
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 3.
#[ignore = "Phase 2 P9 unblocks — ADR-0008 M7 not yet implemented"]
#[tokio::test]
async fn login_synthetic_returns_400() {
    unimplemented!(
        "ADR-0008 Phase 2 P9 deliverable: provider_kind=synthetic → \
         400 invalid_provider_kind."
    );
}

/// POST /api/login WITHOUT the `provider_kind` field → defaults to
/// `Anthropic` → behaves identically to v0.2.x. Locks the back-compat
/// contract for existing tooling / curl scripts.
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 4.
#[ignore = "Phase 2 P9 unblocks — ADR-0008 M7 not yet implemented"]
#[tokio::test]
async fn login_missing_provider_kind_defaults_anthropic() {
    unimplemented!(
        "ADR-0008 Phase 2 P9 deliverable: omit provider_kind from body \
         → server defaults to Anthropic → dispatch uses AnthropicProvider."
    );
}

/// First login: provider_kind=anthropic. Second login with the SAME
/// passphrase + provider_kind=openai → both succeed (wrong-passphrase
/// guard verifies the PASSPHRASE only, not the kind — provider rotation
/// is a legitimate user action).
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 5.
#[ignore = "Phase 2 P9 unblocks — ADR-0008 M7 not yet implemented"]
#[tokio::test]
async fn re_login_changes_provider_kind() {
    unimplemented!(
        "ADR-0008 Phase 2 P9 deliverable: re-login with different \
         provider_kind + same passphrase succeeds; dispatch uses new kind."
    );
}

/// An `EndpointSecret` blob sealed by v0.2.x (no `provider_kind` field
/// in the serialized JSON) deserializes to `provider_kind = Anthropic`
/// when decrypted by an M7 binary. Preserves the M6 → M7 upgrade path
/// without forcing users to re-login.
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 6.
#[ignore = "Phase 2 P9 unblocks — ADR-0008 M7 not yet implemented"]
#[tokio::test]
async fn existing_blob_decryption_supplies_kind() {
    unimplemented!(
        "ADR-0008 Phase 2 P9 deliverable: pre-M7 sealed blob (no \
         provider_kind in JSON) decrypts → secret.provider_kind = Anthropic."
    );
}
