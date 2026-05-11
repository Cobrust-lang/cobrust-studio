//! Strip #5 verification (ADR-0006).
//!
//! The default cache directory must live under the `cobrust-studio`
//! namespace, never under upstream's legacy `.cobrust/`. The exact resolution
//! (`$XDG_DATA_HOME` vs `$HOME/.cache` vs relative fallback) is dependent on
//! the test runner's environment; the namespace assertion is the
//! environment-independent invariant.

use studio_router::Cache;

#[test]
fn default_cache_dir_is_studio_namespace() {
    let path = Cache::default_dir();
    let s = path.to_string_lossy();
    assert!(
        s.contains("cobrust-studio"),
        "expected 'cobrust-studio' in default cache dir, got: {s}"
    );
    assert!(
        !s.contains(".cobrust/"),
        "found upstream-leftover '.cobrust/' in path: {s}"
    );
    assert!(
        s.ends_with("llm_cache"),
        "default dir must end with llm_cache leaf, got: {s}"
    );
}
