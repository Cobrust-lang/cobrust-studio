//! Strip #4 verification (ADR-0006).
//!
//! `LedgerEntry.task_tag` must be `Option<String>` — generic, caller-supplied,
//! never typed to the upstream Cobrust translation-pipeline enum
//! (`spec_extract` / `translate` / `repair` / `L0..L3`).
//!
//! The asserts here exercise both `Some` and `None` cases through the public
//! `LedgerEntry` constructor + serde round-trip so any future change of the
//! field's type or default would break this test.

use studio_router::ledger::now_rfc3339;
use studio_router::{LedgerEntry, Outcome, ProviderKind, TokenUsage};

#[test]
fn ledger_entry_task_tag_is_optional_string() {
    let entry = LedgerEntry::ok(
        now_rfc3339(),
        Some("agent-turn".to_string()),
        "anthropic_official",
        Some(ProviderKind::Anthropic),
        "claude-opus-4-7",
        "blake3:abcd",
        false,
        TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 20,
        },
        100,
        1,
    );
    let entry_no_tag = LedgerEntry::ok(
        now_rfc3339(),
        None,
        "anthropic_official",
        Some(ProviderKind::Anthropic),
        "claude-opus-4-7",
        "blake3:efgh",
        false,
        TokenUsage::default(),
        50,
        1,
    );

    assert_eq!(entry.task_tag.as_deref(), Some("agent-turn"));
    assert!(entry_no_tag.task_tag.is_none());
    assert!(matches!(entry.outcome, Outcome::Ok));

    // JSON round-trip — task_tag must serialise transparently as Option<String>.
    let json = serde_json::to_string(&entry).expect("serialise");
    assert!(
        json.contains(r#""task_tag":"agent-turn""#),
        "task_tag must serialise as a JSON string: {json}"
    );
    let back: LedgerEntry = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(back.task_tag.as_deref(), Some("agent-turn"));

    // None case round-trip — must omit or serialise as null, never as the
    // upstream `"task":"translate"`-style enum-keyed string.
    let json_none = serde_json::to_string(&entry_no_tag).expect("serialise");
    let back_none: LedgerEntry = serde_json::from_str(&json_none).expect("deserialise");
    assert!(back_none.task_tag.is_none());
    assert!(
        !json_none.contains(r#""task":"translate""#),
        "upstream enum-keyed task field must be gone: {json_none}"
    );
}
