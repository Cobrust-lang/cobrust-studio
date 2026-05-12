---
adr_id: "0011"
title: M10 i18n — zh/en UI language toggle
status: proposed
date: 2026-05-12
supersedes: []
superseded_by: []
---

# ADR-0011: i18n — zh/en UI language toggle (M10)

## Context

User dogfood feedback at commit `2846eaa` (2026-05-12 evening):

> "GUI很垃圾啊,没有持久化存储api端点信息,然后也没给出base url需求格式,
> 也没有中英文切换"

Translation: "GUI is trash — no persistence for API endpoint info,
no base URL format guidance, no zh/en toggle."

Of the three:
- **Persistence** — M8 in-flight (ADR-0009 P9 Opus 4.7 dispatch
  `aa99715d362150583`); credentials will auto-unlock on restart.
- **Base URL format hint** — fixed inline at the same commit as
  this ADR landed.
- **zh/en toggle** — this ADR's scope.

The repo already maintains zh/en parity on `docs/human/{zh,en}/`
(F20 + doc-coverage §2 enforce). The frontend (SvelteKit 5) is
English-only. The user is bilingual Chinese-native; English-only
UI is friction. Studio's positioning page (§13) also targets
Chinese-speaking engineering teams as a primary segment.

## Hard constraints

- **No SSR i18n complexity** — Studio's SvelteKit uses
  `@sveltejs/adapter-static`. All routes are pre-rendered at build
  time + served via rust-embed. We do NOT have SvelteKit's `[lang]`
  param routing or SSR-time language detection. Client-side
  reactive switch only.
- **No external translation service** — the workspace must stay
  hermetic. All strings live in source.
- **No runtime locale file fetching** — bundle all locales into
  the static build; rust-embed bakes them into the binary.
- **Backward compat** — existing English text stays as the default
  fallback. Users who don't toggle keep seeing English.
- **No introduction of npm-published i18n frameworks unless
  trivially small** — svelte-i18n / typesafe-i18n / paraglide are
  candidates but each adds bundle weight. A pure-Svelte-store
  implementation is preferred if the string count is < 200.

## Options considered

### Option A — `svelte-i18n` library

Established ecosystem (~150 KB transitive deps), supports JSON
locale files, has Intl integration for date/number formatting.

**Pros**: feature-rich, well-maintained.
**Cons**: bundle weight is significant for a UI with ~50-100
strings total. Studio's binary stays at ~9 MiB; adding 150 KB of
JS for i18n is acceptable but not zero-cost.

### Option B — `paraglide-js` (svelte-flavor)

Inlangs's compile-time i18n with treeshakable per-message exports.

**Pros**: zero runtime overhead, tiny bundle.
**Cons**: newer ecosystem, requires a compile step in `pnpm build`,
adds toolchain complexity. Probably overkill.

### Option C — Custom Svelte 5 store with locale dicts in source

```typescript
// $lib/i18n.ts
import { writable, derived } from 'svelte/store';

type Locale = 'en' | 'zh';
export const locale = writable<Locale>('en');

const messages: Record<Locale, Record<string, string>> = {
    en: {
        'login.title': 'Cobrust Studio',
        'login.tagline': 'Configure your LLM endpoint to start dispatching agents.',
        'login.base_url': 'Base URL',
        // ...
    },
    zh: {
        'login.title': 'Cobrust Studio',
        'login.tagline': '配置 LLM 端点以开始 dispatch agent。',
        'login.base_url': '基础 URL',
        // ...
    },
};

export const t = derived(locale, ($l) => (key: string) => messages[$l][key] ?? key);
```

Components:
```svelte
<script>
  import { t } from '$lib/i18n';
</script>
<span>{$t('login.base_url')}</span>
```

Toggle: stored in `localStorage` + reactive store; survives reload.

**Pros**: zero new deps. Bundle weight = literally the size of the
locale dict object (~5-10 KB for ~100 strings × 2 locales). Tiny.
Trivial to extend. Matches SvelteKit's idiomatic store pattern.
**Cons**: no Intl-aware date/number/plural formatting (Studio
doesn't need any today). Manual key-existence enforcement (typo
in `$t('login.titel')` silently shows `'login.titel'` instead of
text — mitigated by TypeScript narrowing on the dict keys; need a
helper type).

### Option D — Defer to v0.5.x

User said "no zh/en toggle" is a current friction. Deferring
contradicts the directive.

## Decision

**Option C** — custom Svelte 5 store with locale dicts.

### Implementation outline

1. **`web/src/lib/i18n.ts`** (NEW):
   - `Locale` type (`'en' | 'zh'`).
   - `locale: Writable<Locale>` (default `'en'`).
   - `messages: Record<Locale, MessageDict>` (single source of truth).
   - `t: Readable<(key: K) => string>` (typed on dict keys for
     compile-time error on typos).
   - `setLocale(loc: Locale): void` — sets store + persists to
     `localStorage['cobrust-studio-locale']`.
   - `loadLocale(): void` — called from `+layout.svelte` onMount to
     restore from localStorage.

2. **`web/src/lib/i18n/en.ts`** + **`web/src/lib/i18n/zh.ts`** —
   the dicts. Splitting per locale = bundler can tree-shake unused
   locales (small win; both bundles ship together so not strictly
   needed but cleaner).

3. **Top-level layout `web/src/routes/+layout.svelte`** — add a
   small language toggle in a corner (top-right). Two-state
   button (`EN ⇄ 中`). Persists choice. On mount, restores from
   localStorage.

4. **String extraction** — replace English strings in the 5 pages
   (`/login`, `/adr`, `/agent`, `/finding`, `/ledger`) + shared
   components with `{$t('key')}` calls. Initial scope: only the
   user-visible strings; URLs / error codes / debug strings stay
   English. Target ~80-150 keys.

5. **Translation quality** — zh translations done by the maintainer
   (native speaker). NOT machine-translated; we'd rather have 70%
   coverage with accurate translations than 100% with awkward ones.
   v0.4.x ships partial; v0.5.x reaches full.

### Toggle button placement

Top-right corner of every page, in the header area. Renders as:
```
[ EN | 中 ]
```
Highlighted = active locale. Clicking the other side switches.
Plain visible affordance; no settings menu.

### localStorage key

`cobrust-studio-locale`. Value is `"en"` or `"zh"`. Fallback to
`"en"` on missing / invalid.

### Server-side note

The server is locale-agnostic — API error codes/messages remain
English (machine-readable strings; clients render their own
human-readable equivalents). zh translations of error messages
that surface to the user via toast (e.g. "passphrase must be ≥ 8
characters") go in the client-side dict and key off the server's
error `code`.

## Done means

1. **`web/src/lib/i18n.ts`** + `en.ts` + `zh.ts` exist with the
   message store + dict split.

2. **`+layout.svelte`** loads locale on mount + renders the toggle.

3. **All 5 pages** (`/login`, `/adr`, `/agent`, `/finding`,
   `/ledger`) use `$t()` for user-visible strings. Initial scope:
   labels, button text, toast messages, page titles. Defer to
   v0.5.x: ADR/finding markdown body rendering (those are
   user-content, not chrome).

4. **Tests**:
   - `web/src/lib/i18n.test.ts` (Vitest):
     - `default locale is 'en'`.
     - `setLocale('zh') then t('login.title') returns zh translation`.
     - `localStorage persistence: setLocale → reload → restores`.
     - `unknown key falls back to the key string itself`.
   - `web/tests/e2e/i18n.spec.ts` (Playwright):
     - `default page shows English login title`.
     - `clicking the 中 toggle changes the title to zh translation`.
     - `reload preserves zh choice`.

5. **Doc-coverage 7-gate stays green**:
   - `docs/agent/modules/web-frontend.md` (or analogue) updates
     for the new `$lib/i18n` API surface.
   - zh/en parity on `docs/human/{zh,en}/` already maintained; add
     a new `docs/human/{zh,en}/i18n-frontend.md` describing the
     toggle UX + how to add new translation keys.

6. **CHANGELOG entry** for v0.4.0 (or wherever it lands):
   - References ADR-0011 + addresses user dogfood feedback
     2026-05-12.

7. **README** — update §"What's in this repo right now" to note
   bilingual UI (Chinese + English).

## Phase plan

**Phase 1 (this commit, CTO solo)**:
- This ADR landed.

**Phase 2 (P9 dispatch, sonnet 4.6, ~90-120 min — frontend-focused,
no Rust changes; queue post-M8 merge to avoid `/login` worktree
conflict)**:
- Worktree: `feature/m10-i18n-zh-en`.
- Deliverables: i18n.ts + en/zh dicts + layout toggle + 5-page
  string extraction + 4 unit + 3 e2e tests + module-doc updates +
  zh/en human-track doc + README + CHANGELOG.
- 7-gate green; CTO 守闸; merge --no-ff.

## Consequences

- **Enables**: Chinese-native engineering teams (Studio's primary
  target segment per §13 positioning) can use the UI in their
  native language. Reduces "GUI 很垃圾" friction.
- **Enables**: future locales (ja / ko / etc.) by appending dict
  entries. No architectural change needed.
- **Forecloses**: SSR-time locale detection (Studio's static
  adapter doesn't need it; users explicitly toggle). Reopens
  if/when Studio moves off `adapter-static`.
- **Migration**: zero — existing users see English (default);
  toggle is opt-in.
- **Bundle size**: ~5-10 KB JS for the dict + ~1 KB for the
  toggle component. Negligible. Studio binary stays under 10 MiB.

## Cross-references

- ADR-0001 (stack — SvelteKit + adapter-static)
- ADR-0003 (auth — /login is the primary affected page for M10
  user-visible strings)
- ADR-0008 (M7 multi-provider /login — the Provider dropdown
  labels gain Chinese translations)
- `docs/agent/roadmap-v0.4.x.md` (M10 i18n moves from "not on
  list" to "in-flight" status)
- User dogfood feedback at commit `2846eaa` 2026-05-12 evening
