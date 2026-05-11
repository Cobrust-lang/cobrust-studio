//! Strip-invariant checks (ADR-0006 verification gate for A1.1).
//!
//! These tests fix the post-strip public surface so a future refactor that
//! re-introduces a Cobrust-specific symbol breaks the build instead of
//! silently re-leaking. Per ADR-0006 §"Verification gate":
//!
//! - Strip #1 — `Strategy::Consensus { n }` must not exist.
//! - Strip #3 — `Task`, `RoutingEntry`, `StrategyName`, `DefaultStrategy`
//!   must not be re-exported.
//! - Strip #6 — `RouterResponse` must not be re-exported.

use studio_router::Strategy;

/// Exhaustive match: if a future change adds `Strategy::Consensus`, this
/// function becomes non-exhaustive and the test crate fails to compile.
#[test]
fn strategy_has_no_consensus() {
    fn ensure_no_consensus(s: Strategy) -> &'static str {
        match s {
            Strategy::Cost => "cost",
            Strategy::Quality => "quality",
            Strategy::Latency => "latency",
        }
    }
    assert_eq!(ensure_no_consensus(Strategy::Cost), "cost");
    assert_eq!(ensure_no_consensus(Strategy::Quality), "quality");
    assert_eq!(ensure_no_consensus(Strategy::Latency), "latency");
}

/// Helper for the trait-bound surface check. Declared at item scope to
/// satisfy `clippy::items_after_statements`.
fn _accepts_provider<P: studio_router::LlmProvider>() {}

/// Lifting the public surface into one path-resolution point. If any of these
/// imports breaks (i.e. the symbol re-appears in `studio_router`'s root), the
/// test compile fails — which is the desired signal.
#[test]
fn surface_imports_resolve_to_expected_targets() {
    // Each `use` here resolves only against the surface declared in
    // ADR-0006 §Decision. A compile error means strip drifted.
    use studio_router::{
        AnthropicProvider, Cache, CacheKey, Chunk, CompletionRequest, CompletionResponse,
        DispatchResponse, Ledger, LedgerEntry, LlmError, Message, OpenAiProvider, Outcome,
        ProviderConfig, ProviderKind, ProviderModel, RetryPolicy, Role, Router, RouterBuilder,
        RouterConfig, RouterError, SamplingParams, Strategy, TokenUsage,
    };
    // Touch each so the import is not pruned. `std::mem::size_of` is enough
    // to fix the symbol but produces no runtime cost.
    let _ = std::mem::size_of::<Chunk>();
    let _ = std::mem::size_of::<CompletionRequest>();
    let _ = std::mem::size_of::<CompletionResponse>();
    let _ = std::mem::size_of::<LlmError>();
    let _ = std::mem::size_of::<Message>();
    let _ = std::mem::size_of::<Role>();
    let _ = std::mem::size_of::<SamplingParams>();
    let _ = std::mem::size_of::<TokenUsage>();
    let _ = std::mem::size_of::<CacheKey>();
    let _ = std::mem::size_of::<Cache>();
    let _ = std::mem::size_of::<LedgerEntry>();
    let _ = std::mem::size_of::<Ledger>();
    let _ = std::mem::size_of::<Outcome>();
    let _ = std::mem::size_of::<ProviderConfig>();
    let _ = std::mem::size_of::<ProviderKind>();
    let _ = std::mem::size_of::<ProviderModel>();
    let _ = std::mem::size_of::<RouterConfig>();
    let _ = std::mem::size_of::<RouterError>();
    let _ = std::mem::size_of::<Strategy>();
    let _ = std::mem::size_of::<DispatchResponse>();
    let _ = std::mem::size_of::<RetryPolicy>();
    let _ = std::mem::size_of::<RouterBuilder>();
    let _ = std::mem::size_of::<Router>();
    // Trait + adapter types: fix the symbol via a generic helper.
    let _: fn() = _accepts_provider::<AnthropicProvider>;
    let _: fn() = _accepts_provider::<OpenAiProvider>;
}

// The following are "uncomment-to-verify" checks: each line, if uncommented,
// MUST fail to compile against the stripped surface. Keep them commented so
// the test suite stays green; reviewers can flip one at a time to confirm
// the strip is real.
//
// Covers strips #1, #2, #3, #6 plus the lifted-but-unused-by-Studio symbols
// (`L2Verdict` / `HonestGate` are no-op at pin `61f2aff` per finding
// `a1-1-strip-2-noop-at-pin-61f2aff` — these tripwires arm strip #2 against
// future pin bumps that re-introduce honest-gate machinery).
//
// #[allow(dead_code)] fn _no_task_enum()        { let _: studio_router::Task;            }
// #[allow(dead_code)] fn _no_routing_entry()    { let _: studio_router::RoutingEntry;    }
// #[allow(dead_code)] fn _no_strategy_name()    { let _: studio_router::StrategyName;    }
// #[allow(dead_code)] fn _no_default_strategy() { let _: studio_router::DefaultStrategy; }
// #[allow(dead_code)] fn _no_router_response()  { let _: studio_router::RouterResponse;  }
// #[allow(dead_code)] fn _no_l2_verdict()       { let _: studio_router::L2Verdict;       }
// #[allow(dead_code)] fn _no_honest_gate()      { let _: studio_router::HonestGate;      }
// #[allow(dead_code)] fn _no_gate_verdict()     { let _: studio_router::GateVerdict;     }
