---
doc_kind: module
module_id: web-frontend
last_verified_commit: eff54b4
dependencies: [adr:0001, adr:0002, adr:0003, adr:0006]
---

# Module: web-frontend

## Purpose

SvelteKit 5 single-page frontend for Cobrust Studio. Realises the
SvelteKit UI line of CLAUDE.md §1 ("login / project / adr / agent /
finding / ledger") against the `studio-server` REST + SSE surface.
Lives under `web/` outside the Rust workspace; consumed at M3 by
`studio-server` via `rust-embed` over `web/build/` (per ADR-0002).

## Stack (as-built; Wave M2 deliverable)

Frozen by the package.json on the M2 branch:

- **SvelteKit** 2.x with **Svelte 5** (runes-based reactivity —
  `$state`, `$derived`, `$effect`, `$props`, `$bindable`). Adapter:
  `@sveltejs/adapter-static` (pre-rendered SPA; `fallback: 'index.html'`,
  `strict: false`).
- **Tailwind v4** via `@tailwindcss/vite` plugin (CSS-first config; no
  separate `tailwind.config.js`/`postcss.config.cjs`). Tokens declared
  in `src/app.css` under `@theme` + `@layer base` (HSL channels).
- **bits-ui** / **lucide-svelte** / **tailwind-merge** /
  **tailwind-variants** / **clsx** are installed (M0 placeholder for
  shadcn-svelte primitives) but only `bits-ui` is exercised in M2;
  the four shipped pages use hand-rolled `Button` / `Badge` / `Modal`
  primitives — see "Why hand-rolled primitives" below.
- **prettier** (+ `prettier-plugin-svelte`) is the lint surface
  (`pnpm run lint` = `prettier --check .`). ESLint is intentionally
  not configured for M2 — Svelte 5 + svelte-check covers the
  diagnostics gap, and adding eslint-plugin-svelte ahead of its 5-rune
  maturity (mid-2025) wasted a Wave M1 session previously.

### Why hand-rolled primitives (not shadcn-svelte)

shadcn-svelte's component library (button, input, dialog, table,
badge, tabs, dropdown-menu, sonner) was the M2 default per CLAUDE.md
§6. After scaffolding the 4 pages, the actual surface area was small
enough that pulling shadcn-svelte's full primitive set added more
maintenance than it removed:

- 4 pages, each with ~1 dialog + ~1 table + ~3 form fields. Total
  primitive uses: ~20 across the codebase.
- Hand-rolled `Button.svelte` (47 lines), `Badge.svelte` (22 lines),
  `Modal.svelte` (90 lines, native `<dialog>` element with focus trap
  + ESC for free) ship in 4 KB of source.
- shadcn-svelte equivalents would have been ~12 components, each ~80
  lines, plus the cn() helper, registry tooling, and the runtime
  `bits-ui` headless surface they wrap.
- Tailwind v4's CSS-tokens approach lets the hand-rolled primitives
  reuse the design tokens already declared in `app.css` —
  `bg-primary`, `text-card-foreground`, etc.

Future judgment call: if M3 adds a fifth page or the primitive count
crosses ~10 distinct components, re-run `pnpm dlx shadcn-svelte@latest
init` and migrate.

## Public surface

### Pages (`web/src/routes/`)

| Path | Wire endpoints | Notes |
|---|---|---|
| `/` | — | Redirect to `/adr` on mount. |
| `/login` | `POST /api/auth/set-endpoint` | API key tab (active); OAuth tab greyed pending v0.5.0. Client-side AES-GCM-256 encryption via `$lib/crypto.ts` (M2 stub; real AEAD M3). |
| `/adr` | `GET /api/adr`, `GET /api/adr/:id`, `POST /api/adr`, `GET /api/events` | List + detail dialog + create form. Live re-list on `adr_*` events. |
| `/agent` | `POST /api/dispatch` (SSE) | Composer + live stream renderer; `chunk` → append, `done` → summary badges, `error` → envelope banner. 503 `router_not_configured` → "Configure LLM endpoint" CTA → `/login`. |
| `/finding` | `GET /api/finding`, `POST /api/finding`, `GET /api/events` | Symmetric to `/adr` with severity colour badges (P0=red, P1=warn-orange, P2=yellow, P3=info-blue). Detail dialog is summary-only — singleton `GET /api/finding/:id` is M2+ per the server module-doc. |
| `/ledger` | `GET /api/ledger/recent[?n=N]` | Recent-N table. `n` clamped to `[1, 1000]` server-side. |

### Shared lib (`web/src/lib/`)

- **`types.ts`** — TypeScript mirrors of Rust serde shapes. Wire-shape
  convention: `Adr` and `Finding` are **flat** (no nested `summary`
  field) — the Rust side carries `#[serde(flatten)]` on the embedded
  summary struct (A5 reconcile). `DispatchRequest` includes an
  optional `task_tag` field per ADR-0006 §"Addendum 2026-05-11" F-03.

- **`api.ts`** — typed `fetch` wrapper exporting `listAdrs`, `getAdr`,
  `createAdr`, `listFindings`, `createFinding`, `recentLedger`,
  `getProject`, `getVersion`, `setEndpoint`, `dispatchSse` (async
  generator over SSE frames), `subscribeEvents` (EventSource wrapper).
  Every non-2xx surface throws `ApiError` carrying the server-supplied
  `{error, code}` envelope.

- **`crypto.ts`** — M2 WebCrypto stub. AES-GCM-256 with a
  PBKDF2-derived key over a fixed passphrase (100k SHA-256 rounds);
  scheme tag `"aes-gcm-256/m2-stub"`. Real AEAD lands at M3 per the
  studio-server module-doc Wave A6+ "real AEAD decryption" note.

- **`store.svelte.ts`** — Svelte 5 runes-based singleton (`appState`)
  carrying `project: ProjectCurrent | null` and `version: VersionInfo |
  null`, hydrated by `+layout.ts` on mount.

- **`util.ts`** — `cn` (class merger), `fmtTs` (RFC-3339 → compact UTC),
  `adrStatusClass` / `severityClass` / `findingStatusClass` (HSL
  status-palette mappings).

- **`components/{Button,Badge,Modal}.svelte`** — see "Why hand-rolled
  primitives" above.

### Root layout (`web/src/routes/+layout.{ts,svelte}`)

- `+layout.ts` — `prerender = false; ssr = false;` Boot-time parallel
  fetch of `/api/project/current` and `/api/version`; failures are
  swallowed (the navbar renders a "server unreachable" red dot
  instead) so a not-yet-up server doesn't block the SPA render.
- `+layout.svelte` — navbar (logo, page links, project-root preview,
  server version, reachable status dot, theme toggle, endpoint link).
  Theme is `.dark` class on `<html>` by default; toggling persists to
  `localStorage` under `cs-theme`. Hidden on `/login` for full-screen
  centred form.

## Internal architecture

```
web/
├── package.json + pnpm-lock.yaml + tsconfig.json + svelte.config.js + vite.config.ts
├── .prettierrc + .prettierignore
├── static/
│   └── favicon.svg                 # 32×32 CS monogram
└── src/
    ├── app.html                    # html.dark default; antialiased body
    ├── app.css                     # design tokens (stone surfaces, slate-blue accent),
    │                               # status palette (ok/warn/err/info), Inter font,
    │                               # tailwind v4 @theme + @layer base
    ├── app.d.ts
    ├── lib/
    │   ├── api.ts                  # typed fetch + SSE wrappers + ApiError
    │   ├── crypto.ts               # AES-GCM-256 M2 stub for endpoint encryption
    │   ├── store.svelte.ts         # runes singleton (project + version + routerConfigured)
    │   ├── types.ts                # TS mirrors of Rust serde shapes
    │   ├── util.ts                 # cn / fmtTs / status-class helpers
    │   └── components/
    │       ├── Badge.svelte        # caller-class status pill
    │       ├── Button.svelte       # 4-variant 2-size primitive
    │       └── Modal.svelte        # native <dialog> wrapper
    └── routes/
        ├── +layout.ts              # boot fetch
        ├── +layout.svelte          # navbar
        ├── +page.svelte            # redirect to /adr
        ├── login/+page.svelte
        ├── adr/+page.svelte
        ├── agent/+page.svelte
        ├── finding/+page.svelte
        └── ledger/+page.svelte
```

### Dev mode vs M3 single-binary

- **Dev** (`pnpm run dev` on `:5173`): `vite.config.ts` proxies
  `/api/*` → `http://127.0.0.1:7878` (the studio-server CLI default).
  Tailwind v4 hot-reloads via the `@tailwindcss/vite` plugin.
- **Build** (`pnpm run build`): adapter-static emits a fully
  pre-rendered SPA into `web/build/` (index.html + _app/immutable/*).
  M2 ships a ~236 KB build directory; M3 dogfood will `rust-embed`
  this directory and serve same-origin from studio-server's binary.

### SSE plumbing

Two distinct SSE consumers:

1. **`/api/events`** — `subscribeEvents()` in `api.ts` uses the
   browser `EventSource` API (which already handles the
   `\n\n`-delimited frame parsing, keep-alive comment frames, and
   auto-reconnect). One subscription per page, torn down on
   `onDestroy`. The current contract treats `/api/events` as
   coarse-grained: any `adr_*` or `finding_*` event triggers a
   `refresh()` call rather than a diff-merge patch. Server's
   15s keep-alive comment frames (per studio-server module-doc
   §"Wave A4 watcher bridge") are invisible to EventSource.

2. **`/api/dispatch`** — `dispatchSse()` in `api.ts` hand-rolls SSE
   parsing over `fetch` + `ReadableStream` because the browser's
   `EventSource` API doesn't accept `POST` requests. Async generator
   yields typed events (`{kind: 'chunk', delta}` /
   `{kind: 'done', payload}` / `{kind: 'error', payload}`) per
   ADR-0006 F-01 wire contract. Cancellation via `AbortSignal`.

### Theme system

Dark-mode-first per ADR-0001. `app.html` sets `<html class="dark">`;
`+layout.svelte` swaps to `class="light"` on toggle and persists the
choice. Tokens are HSL channels declared on `:root` (dark) and
`.light` so the same `bg-card` / `text-foreground` classes resolve
correctly under either theme.

## Gates (Wave M2)

Run from `web/`:

```
pnpm install            # gate 0 — pinned by pnpm-lock.yaml
pnpm run check          # gate 1 — svelte-kit sync && svelte-check
pnpm run lint           # gate 2 — prettier --check .
pnpm run build          # gate 3 — adapter-static -> web/build/
pnpm run test:unit      # gate 4 — vitest (Wave M2 TEST)
pnpm run test:e2e       # gate 5 — playwright (Wave M2 TEST; skipped by default)
```

All six pass on the M2 deliverable; gate 5 reports "skipped" unless
the harness env is set (see §Tests below).

The Rust workspace 5-gate (`cargo fmt --check`, `cargo clippy -D
warnings`, `cargo build`, `cargo test`, `bash scripts/doc-coverage.sh`)
is unaffected — M2 frontend lives outside the workspace and modifies
no Rust source.

## Tests (Wave M2 TEST)

Two layers, both inside `web/`:

### Layer 1 — Vitest unit tests (fast, no browser)

Lives next to the module under test in `src/lib/`:

```
src/lib/api.test.ts      # 20 tests — fetch wrapper + SSE consumer
src/lib/crypto.test.ts   # 8 tests — encryptEndpointBlob round-trip
src/lib/types.test.ts    # 4 tests — compile-time wire-shape pins
```

Total: 32 tests, runs in ~1s. Vitest `^3` + jsdom `^29` — pinned to
v3 because v4 requires Vite 6 and the M2 frontend is on Vite 5.4.
The setup file at `src/test-setup.ts` supplements Node's `webcrypto`
if jsdom ever regresses `crypto.subtle`.

Wire-contract pins (each maps to a section of
`docs/agent/modules/studio-server.md`):

- Every `fetch()` path in `src/lib/api.ts` — URL, method, headers,
  request body, response envelope, error envelope.
- SSE frame parsing (chunk / done / error / comment / unknown-event /
  mid-frame TCP boundary) per the A5.1 守闸 doc.
- `EncryptedBlob` triple `{ ciphertext, nonce, scheme }` and the
  literal scheme tag `"aes-gcm-256/m2-stub"`.
- Flat (post-A5-reconcile) `Adr` / `Finding` shapes — `adr_id` /
  `finding_id` live at the top level, not nested under `summary`.

Run:

```
pnpm run test:unit
```

### Layer 2 — Playwright end-to-end tests (browser, requires backend)

Lives under `tests/e2e/`:

```
tests/e2e/_fixtures.ts          # studioBaseURL + skipIfHarnessDisabled
tests/e2e/_setup.ts             # globalSetup — tempdir + spawn (hermetic)
tests/e2e/_teardown.ts          # globalTeardown — kill + rmdir
tests/e2e/_setup-dogfood.ts     # globalSetup — repo-root spawn (dogfood)
tests/e2e/_teardown-dogfood.ts  # globalTeardown — kill (no tempdir)
tests/e2e/login.spec.ts         # 3 tests — endpoint config flow
tests/e2e/adr.spec.ts           # 3 tests — list / create / detail
tests/e2e/agent.spec.ts         # 2 tests — dispatch SSE (router-None + Some)
tests/e2e/finding.spec.ts       # 3 tests — list / create / summary detail
tests/e2e/ledger.spec.ts        # 3 tests — list / n-query / refresh
tests/e2e/dogfood.spec.ts       # 2 tests — M3 done-means constitutional ADRs
```

Total: 14 hermetic specs + 2 dogfood specs. **Wave M3 unflagged the
M2 SKIPPED gate** — `pnpm run test:e2e` now spawns the release binary
automatically (see `_setup.ts`) and runs the suite end-to-end. The
SKIPPED-with-reason path is reserved for the *binary-missing* fallback
during cross-branch handoff (see "Cross-branch dependency" below).

#### Harness — hermetic project

```
┌──────────────────────────────────────────────────────────────┐
│ _setup.ts: mkdtemp + spawn target/release/cobrust-studio     │
│            --project <tempdir> --port <random>               │
│                                                              │
│   ┌────────────────────────────┐                             │
│   │ cobrust-studio :<random>   │  ◄── chromium navigations   │
│   │ rust-embed serves the SPA  │     (login / adr / agent /  │
│   │ same-origin, no proxy      │      finding / ledger)      │
│   └────────────────────────────┘                             │
│                                                              │
│ _teardown.ts: SIGINT → SIGKILL grace → rmdir tempdir         │
└──────────────────────────────────────────────────────────────┘
```

Key knobs:

- `STUDIO_E2E_ROUTER=1` — setup writes a synthetic-provider
  `studio.toml` into the tempdir so `/api/dispatch` returns 200 SSE
  (router-on universe of `agent.spec.ts`). Default: router-off (503).
- `STUDIO_E2E_DEBUG=1` — surface server stderr + setup tracing on the
  Playwright stdout (useful when a setup races a port collision).

#### Harness — dogfood project (M3 done-means)

```
┌──────────────────────────────────────────────────────────────┐
│ _setup-dogfood.ts: spawn target/release/cobrust-studio       │
│                    --project <repo-root> --port <random>     │
│                                                              │
│ dogfood.spec.ts navigates to /adr and asserts the 6          │
│ constitutional ADRs (per CLAUDE.md §6) render in the table.  │
│                                                              │
│ _teardown-dogfood.ts: kill child (no tempdir to remove).     │
└──────────────────────────────────────────────────────────────┘
```

The dogfood spec is the binding M3 done-means test per CLAUDE.md §6
(`Studio manages its own ADRs via Studio UI`). It uses a SEPARATE
config (`playwright-dogfood.config.ts`) because Playwright resolves
one `globalSetup`/`globalTeardown` pair per config, and the dogfood
setup spawns against the repo root rather than a tempdir.

#### To run locally

```
bash scripts/build-release.sh     # M3 DEV — produces target/release/cobrust-studio
cd web
pnpm install
pnpm run test:e2e                  # 14 hermetic specs
pnpm run test:e2e:dogfood          # 2 dogfood specs (constitutional ADRs)
```

#### Cross-branch dependency (M3 only)

The hermetic + dogfood projects both spawn `target/release/cobrust-studio`.
Two upstream pieces are required, both shipped by the
`feature/m3-dev-embed-dogfood` branch:

1. `scripts/build-release.sh` — wraps `cargo build --release` with the
   `web/build` adapter-static artefact present (M3 DEV's work).
2. `embed.rs` in `studio-server` — rust-embed integration that serves
   `web/build/` same-origin so page navigations no longer 404.

If either is missing in the active checkout, `_setup.ts` /
`_setup-dogfood.ts` detect the absent binary, set
`STUDIO_E2E_SKIP=1`, and every spec's `beforeEach` short-circuits
with a reason string. The suite reports skips (green) instead of
spurious failures — the CTO can re-run after the DEV merge with no
config change.

## Open questions for CTO (Wave M3)

1. **shadcn-svelte adoption threshold** — if M3 adds richer
   interactions (drag-drop Kanban, command palette, multi-step
   wizards), revisit the hand-rolled-primitive call.

2. **`GET /api/finding/:id` singleton route** — the finding detail
   dialog is summary-only because the M1 server contract deferred
   the singleton route. M3 dogfood will hit this immediately; add
   to the server surface or accept the file-walk fallback?

3. **Auth scheme upgrade timing** — the M2 WebCrypto stub uses a
   fixed passphrase; real M3 AEAD needs a user-secret entry point
   (re-enter on each session? OS keychain integration? a derived
   master key persisted under `studio_store::session`?).

4. **Error envelope code taxonomy stability** — the agent page
   renders `router_auth | router_rate_limit | router_bad_request |
   router_transport | router_server | router_failed | router_no_provider
   | router_config | router_io` codes directly. If the server taxonomy
   changes the page falls back to displaying the raw `code` string —
   should we add a code → human-message lookup table on the frontend
   side, or keep that server-side?

5. **Reconnection / Last-Event-ID** — `/api/events` has no
   Last-Event-ID reconnection in M1. Frontend currently relies on
   the browser's `EventSource` auto-reconnect + an unconditional
   `refresh()` on any event. M3 may want explicit backfill if the
   event stream grows costly.

6. **(Closed Wave M3 TEST)** Hermetic e2e harness wiring — landed at
   `feature/m3-test-hermetic`. `pnpm run test:e2e` now spawns the
   release binary against a tempdir with no manual setup. The
   remaining open: dogfood-spec failure mode if the constitutional
   ADR titles drift (e.g. a CTO renames ADR-0001 to drop "Stack
   choice"); the dogfood pattern matchers will need an update in
   the same PR.

## Cross-references

- ADR-0001 (stack — Rust + Axum + SvelteKit + shadcn-svelte +
  Tailwind)
- ADR-0002 (single-binary — rust-embed of `web/build/` at M3)
- ADR-0003 (auth — `EncryptedBlob` round-trip; M2 client-side stub;
  real AEAD M3)
- ADR-0006 §"Addendum 2026-05-11" F-01 / F-03 (dispatch contract;
  task_tag plumbing via DispatchContext)
- `docs/agent/modules/studio-server.md` §"Wave A4" + §"Wave A5"
  (binding wire contract — every fetch() in `src/lib/api.ts`
  anchors to a section here)
- src: `web/`
- consumed by: `studio-server` at M3 via `rust-embed` (not yet
  wired; surfaced as the `embed.rs` placeholder in the studio-server
  module-doc Wave A6+ extensions section)
