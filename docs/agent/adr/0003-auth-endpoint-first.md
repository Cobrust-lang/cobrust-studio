---
adr_id: "0003"
title: Auth — custom-endpoint-first, OAuth deferred
status: accepted
date: 2026-05-11
supersedes: []
superseded_by: []
---

# ADR-0003: Auth strategy

## Context

MVP must let users start using Studio within minutes. OAuth flows
(Anthropic, OpenAI, GitHub) require: app registration, redirect URL
configuration, review process, ongoing maintenance. Anthropic's OAuth
flow is recent (mid-2025) and still evolving.

A custom-endpoint mode (paste `base_url + api_key + model`) covers:
- Self-hosted vLLM / Ollama users
- DeepSeek / Together / OpenRouter / Groq
- Anthropic API key direct (no OAuth)
- OpenAI API key direct
- Internal corporate proxies

This is exactly the same pattern `cobrust.toml`'s `[providers.X]`
section already uses (see Cobrust ADR-0004).

## Options considered

### Option 1 — Custom-endpoint-first; OAuth deferred to M5

Login screen has two tabs:
- "API key" tab: `base_url` text field + `api_key` password field +
  `model` text field. Stored client-side, encrypted via WebCrypto.
- "OAuth" tab: greyed out with "Coming in v0.5.0" badge.

### Option 2 — OAuth first

Block MVP until Anthropic/OpenAI OAuth is wired. Adds ~1 week.

### Option 3 — No auth in MVP (local-only mode)

Bind to `127.0.0.1`, trust localhost. Acceptable for solo developer
mode but breaks the "login then use" UX framing.

## Decision

**Option 1**. Custom-endpoint-first.

Rationale:
1. Hits 5-day target.
2. Covers 100% of users who could plausibly use Studio today (anyone
   with any LLM endpoint).
3. OAuth is a UX nicety, not a capability gap.

Security: API keys never leave the client unencrypted. Server stores
only encrypted blobs keyed by user-supplied passphrase (or random
per-session key in solo mode).

## Consequences

- Enables: Day 1 MVP delivery.
- Forecloses: zero-config "Sign in with Anthropic" experience until M5.
- Migration: M5 adds OAuth provider behind feature flag; existing
  custom-endpoint configs continue working.

## Cross-references

- ADR-0004 (storage — credentials)
- Cobrust `cobrust.toml.example` for the canonical `[providers.X]` schema
