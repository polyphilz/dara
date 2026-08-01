# Frontend test authority

Dara keeps each assertion at the smallest layer that can tell the truth. Test files are classified
by their authority rather than by how much application code they happen to render.

## Pure/domain

- `tests/scheduling/` and `tests/review/` run in Node after strict TypeScript compilation.
- Pure files beneath `tests/ui/` cover parsers, geometry calculations, zoom mapping, activity
  calculations, cache behavior, and gateway serialization. They use Vitest for organization but
  do not claim browser behavior.

## jsdom semantic components

- Component tests beneath `tests/ui/` own roles, labels, ARIA state, callbacks, validation,
  loading/error states, listener cleanup, and React state transitions.
- `tests/ui/contracts/tauri-gateways.test.ts` uses Tauri's official `mockIPC` implementation and
  owns exact command/payload serialization.

## Browser-fidelity candidates

The following existing areas intentionally use the synthetic browser APIs installed by
`tests/setup.ts`. Their semantic assertions remain useful, but geometry, focus, selection,
pointer, clipboard, overflow, and appearance claims require Playwright counterparts:

- rich-text and CodeMirror tests in `tests/ui/markdown/`;
- image-occlusion editor and picker tests in `tests/ui/occlusion/`;
- keyboard/focus loops in Quick Add, Main Window, Browse, Settings, and shared controls;
- SVG range rectangles, pointer capture, `elementFromPoint`, `ResizeObserver`, scrolling, and
  animation-frame behavior anywhere in the jsdom suite.

Browser tests must not delete the semantic test below them merely because the real-browser proof
exists. Native tests separately own real Rust IPC, databases, WKWebView, persistent windows,
menus, and AppKit activation/focus behavior.

## Commands and gates

| Command | Intended use |
| --- | --- |
| `pnpm test` | Fast Node domain and jsdom semantic loop. |
| `pnpm test:coverage` | Diagnostic V8 coverage artifact; there is no gameable global threshold. |
| `pnpm test:properties` | Fixed PR seed (`20260717`) for reproducible review, cloze, and geometry invariants. |
| `pnpm test:browser` | Chromium journeys plus the focused WebKit contracts. |
| `pnpm test:a11y` | Focused axe scans (also included by the Chromium browser project). |
| `pnpm test:visual` | Canonical DPR-1 and focused DPR-2 comparisons. |
| `pnpm test:visual:update` | Explicit baseline regeneration inside the pinned Playwright container only. |
| `pnpm test:native` | Serial feature-gated native suite with a new database below `.data/e2e/`. |
| `pnpm test:bundle-safety` | Ordinary production build plus structured module-graph/output isolation assertions. |
| `pnpm check` | Ordinary local frontend gate; excludes Linux-canonical visual comparisons. |
| `pnpm release:build:app` | Build, stage, verify, bundle, ad-hoc sign, and inspect the pinned arm64 release app. |
| `pnpm release:verify-app` | Recheck an already-built `.app` without rebuilding llama.cpp. |
| `pnpm release:build:distribution` | Rebuild and verify the pinned sidecars, Developer ID sign every executable with hardened runtime, notarize and staple the app and DMG, then run Gatekeeper checks against the mounted artifact. |
| `pnpm release:resume:distribution` | Resume saved Apple submissions and existing signed artifacts after a transient notarization or stapling failure, without rebuilding or uploading them again. |
| `pnpm release:verify-distribution -- <app> <dmg>` | Recheck an existing signed and notarized public artifact without rebuilding sidecars. |
| `pnpm release:acceptance help` | Drive clean-first-run and previous-schema upgrade acceptance against isolated `.data/` directories and the packaged app. |

Canonical screenshots use
`mcr.microsoft.com/playwright:v1.61.0-noble@sha256:57b65fdc9ceabe0ef613124c7bbe2babcf9362c4d85e382fe3b03604e84b428a`.
CI never updates baselines. Run `test:visual:update` in that exact image, inspect every changed
expected image at full size, and commit only an intentional reviewed delta.

The files in `tests/bundle/baselines/` are the historical before/after size and module-graph
record required by plan 008. They document the reviewed infrastructure delta; they are not moving
size thresholds and are intentionally not rewritten by routine builds.

## Failure triage

- Re-run the smallest named command first. Playwright traces live in `test-results/playwright/`
  and the HTML report in `playwright-report/`.
- A retry-pass is a failure: `failOnFlakyTests` is enabled. Do not increase a timeout or screenshot
  tolerance until the uncontrolled clock, readiness signal, animation, selector, or leaked task is
  identified.
- Property failures print the seed, run count, and fast-check shrink path. Replay with
  `DARA_PROPERTY_SEED=<seed> DARA_PROPERTY_RUNS=<runs> pnpm exec vitest run tests/ui/properties`.
- Native logs are written beneath `logs/`, and `test-results/native/run.json` records the isolated
  database directory. Never redirect a native failure investigation to Dara's platform data
  directory and never delete an existing `.data/` run.
- Screenshot actual/diff files and all runner reports are diagnostics, not new expectations.

## Adding or changing UI behavior

Name the contract first. Pure mapping/parser/reducer branches get unit examples and properties
where an invariant exists. React semantics stay in jsdom. CSS, focus, selection, pointer geometry,
contenteditable, and complete frontend journeys go to Playwright. Real commands, databases,
WKWebView, native windows, menus, and activation policy go to the serial native suite.

Shared controls require semantic tests, a real keyboard/focus test, and the closest applicable
light/dark/open/disabled/error catalog states. A fixed regression test must fail on the buggy
revision. Screenshots do not replace behavior assertions, and native smoke does not duplicate
state-space already covered below it.

## Empirical promotion gates

The scheduled workflow repeats Chromium journeys under one and half-CPU worker schedules and runs
three reported property seeds. The macOS WDIO job remains informational until the embedded driver
has completed 30 consecutive clean runs with no timeout or retry-pass. Quick Add's reference slice
similarly needs a 20-run clean burn-in before browser CI is treated as stable evidence. External
focus restoration, Spaces/fullscreen, sleep/wake, monitor movement, and true IME/dead-key behavior
remain the short checklist in `tests/native/system-smoke.md`.
