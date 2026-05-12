---
doc_kind: finding
finding_id: m4-release-readiness-spa-fallback-extractor
last_verified_commit: a722e09
discovered_by: CTO 守闸 (studio-cto-session-002-opus47) running M4 release-readiness audit post-v0.1.0-tag via hermetic Playwright harness; M3 review forecast all-pass was wrong
severity: P0
status: closed_by_v0.1.1
dependencies: [adr:0002]
related: [cto-shougate-test-gate-grep-leak]
---

# Finding: SPA fallback handler used `Path<String>` extractor; broke every non-root client-side route in v0.1.0

## Hypothesis

The M3 rust-embed integration (commit `5685f49`) mounted
`embed::serve_asset` via `axum::Router::fallback(...)` to handle SPA
client-side routes (`/login`, `/adr`, `/agent`, `/finding`,
`/ledger`). The handler signature was:

```rust
pub async fn serve_asset(Path(path): Path<String>) -> Response { ... }
```

The hypothesis: `axum::extract::Path<T>` extracts named parameters
from a matched route pattern; `Router::fallback` does NOT match a
pattern — it's a catch-all. So `Path<String>` has nothing to extract
from. Every request to a SPA route would fail with the Axum runtime
error "Wrong number of path arguments for `Path`. Expected 1 but
got 0."

## Method

Empirical: ran hermetic Playwright (`STUDIO_E2E=1 pnpm run test:e2e`)
against `./target/release/cobrust-studio` built from main HEAD
`a722e09` (which == v0.1.0). 13 of 14 e2e specs failed at the first
`page.goto('/login')` step.

Inspection of the Playwright `error-context.md`:

```yaml
- text: "Wrong number of path arguments for `Path`. Expected 1 but got 0. Note that multiple parameters must be extracted with a tuple `Path<(_, _)>` or a struct `Path<YourParams>`"
```

This is Axum's exact "use the wrong extractor on a fallback handler"
error string. Hypothesis confirmed.

## Result

**v0.1.0 ships with a critical SPA-routing bug.** Every navigation
to `/login`, `/adr`, `/agent`, `/finding`, `/ledger` returns the
Axum error text as the response body, not the SvelteKit index.html
shell. The frontend is unreachable. The bug was hidden from prior
audits because:

1. `scripts/smoke-dogfood.sh` only tests `GET /` (which uses
   `embed::serve_index`, a separate handler with no extractor) and
   `GET /api/*` paths (which never reach the embed fallback). It
   never exercises a SPA route through the binary.
2. `embed::serve_asset` collocated unit test passed a Path-extracted
   literal directly (`serve_asset(Path("adr/3".to_string())).await`)
   instead of going through the Axum router — so the extractor
   plumbing was never exercised in the unit test either.
3. M3 review forecast (`studio-review-wave-m3-opus47`) said the
   13/14 prior-fail state was "rust-embed not on TEST branch yet;
   post-merge all 14 will pass." This was wrong — the bug is in
   the rust-embed integration's extractor choice, not in branch
   merge state.
4. M4 TEST agent returned mid-flight without running Playwright;
   CTO 守闸 ran the audit directly and caught the bug.

## Conclusion

**Actionable takeaway**: `Path<T>` extractor on a fallback handler
is a structural Axum bug pattern. The replacement is
`axum::http::Uri` (the request's raw URI), from which `.path()`
yields the requested path string:

```rust
use axum::http::Uri;

pub async fn serve_asset(uri: Uri) -> Response {
    serve_path(uri.path())
}
```

Applied at v0.1.1. Re-tested: `curl http://127.0.0.1:.../login`
returns HTTP 200 + text/html + 1677 bytes (the index.html shell).
Playwright recovers from 13 failed → 2 failed (the 2 remaining
are UI text-matcher drift, not protocol-level; tracked separately).

**Locked against regression**:
- New unit test `serve_asset_handles_spa_routes_login_agent_etc`
  exercises every SPA route through the fixed `Uri` extractor.
- Pre-existing `serve_asset_falls_back_to_index_for_unknown_path`
  updated to pass `Uri` instead of `Path`.

**Forward implications**:

- The smoke-dogfood.sh script SHOULD probe a SPA route (e.g.,
  `curl /login | grep '<html'`) to catch this class of regression
  at the script level. Filed for v0.1.2.
- M4 release-readiness pattern: the F19 mandate "any public-facing
  install / quickstart / release command must pass independent
  execution in a clean shell" implicitly extends to "any public
  ROUTE must be hit by an independent caller before publish."
  smoke-dogfood.sh covers /api/* and /; v0.1.1 forward should
  cover SPA routes too.
- M3 review forecast was wrong because it didn't simulate the
  Playwright execution path mentally. ADSD §"Deep-source-read"
  dimension might catch this in future — flag for the catalogue.
- v0.1.0 tag is immutable per F19 §"Recovery". v0.1.1 supersedes.
  CHANGELOG.md documents v0.1.0 as known-broken with explicit
  "upgrade to v0.1.1" note.

## Cross-references

- ADR-0002 (single-binary deployment via rust-embed — the binding
  ADR M3 implements)
- ADSD v1.2.1 §F19 (release-readiness audit; the F19 mandate this
  finding's discovery validates)
- src: `crates/studio-server/src/embed.rs` (fix location)
- v0.1.0 tag: `0a7fd3e` (known-broken; SPA routes return Axum error
  text)
- v0.1.1 tag: TBD (this finding's closure; SPA routes return
  index.html shell)
- M3 review forecast that missed it: out-of-band working notes
  `/tmp/cobrust-studio-review-wave-m3.md` (if retained)
