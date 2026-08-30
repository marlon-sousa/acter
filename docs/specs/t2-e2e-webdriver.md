# Spec: PR T2 — end-to-end tests via WebdriverIO Tauri service

Second PR of the testing-infrastructure track. Drives the real built app —
WebView2, real DOM, real accessibility tree — over the WebDriver protocol. This is
the ground line for automated UI testing: NVDA behavior stays manual, but DOM
semantics regressions (roles, names, live regions) become machine-caught.

## Background (investigated 2026-07-16)

- `tauri-driver` supports Windows and Linux (no macOS); on Windows it drives
  WebView2 via Edge WebDriver.
- The recommended client is **WebdriverIO with the Tauri service**
  (`@wdio/tauri-service`), which offers an **embedded WebDriver provider** (server
  runs inside the app — no external driver binary to version-match) and, on the
  tauri-driver route, keeps Edge WebDriver in sync automatically. It also exposes
  `browser.tauri.execute()` for backend access and IPC mocking.
- Official CI guidance exists for Windows runners (Edge Driver via msedgedriver
  tool; Linux via webkit2gtk-driver + xvfb).

## Amendment (2026-07-16, rides in the implementing PR): direct in-app WebDriver

The implemented architecture differs from the spec above in one structural way:
**WebdriverIO talks directly to a WebDriver server embedded in the app**
(`tauri-plugin-wdio-webdriver`), with no `@wdio/tauri-service`, no `tauri-driver`,
and no Edge WebDriver in the session path. Each spec file's worker spawns its own
app instance on a unique port (`beforeSession`) and kills it afterwards
(`afterSession`) — full per-spec process isolation, parallelizable later by raising
`maxInstances`. Recorded so none of the discarded routes is relitigated:

- **External provider (tauri-driver + msedgedriver), tried first:** worked but
  high-friction — a cargo-installed driver plus a version-matched msedgedriver on
  PATH, and sessions that intermittently hung mid-test. Dropped.
- **`@wdio/tauri-service` with the embedded provider, tried second:** the service
  spawns ONE app for the whole run and points every worker's session at it, so
  specs contaminated each other's DOM state; and its session layer silently probes
  for an optional companion plugin (`tauri-plugin-wdio`) before most commands,
  burning 5-second timeouts — twice per element lookup — until lookup-heavy specs
  blew their budgets. Dropped from the session path; the in-app server it relies on
  is a complete W3C endpoint that plain `webdriverio` drives directly.
- **Companion plugin `tauri-plugin-wdio` deliberately not installed:** it exists
  for `browser.tauri.execute()`/IPC mocking and log forwarding, and requires a
  frontend script in the shipped bundle plus `withGlobalTauri: true` in production
  config. Poor trade for scenarios that never mock IPC. Revisit trigger: if A3+
  wants IPC mocking or log forwarding in E2E, re-evaluate with a concrete need.
- **Gating decision (app side):** `tauri-plugin-wdio-webdriver` registration is
  `#[cfg(debug_assertions)]` only — release binaries never start the automation
  server. (Cargo cannot gate the *dependency* on debug_assertions — target cfg
  tables never evaluate it — so the crate stays a normal dependency and the gate
  lives in the composition root. The `wdio-webdriver:default` ACL entry is inert
  when the plugin is not registered.) Debug builds exist only on developer machines
  and CI.
- **Key synthesis limitation:** the in-app executor dispatches keys as synthetic
  (untrusted) KeyboardEvents, which cannot trigger the browser's native implicit
  form submission. Specs submit via `form.requestSubmit()` (fires the same
  cancelable submit event a real Enter produces); real keystrokes stay manual-NVDA
  territory.

Discovered en route, and load-bearing: Tauri keys dev-vs-embedded assets on the
`custom-protocol` cargo feature, **not** on the build profile. Without the feature,
any built binary (debug or release) runs in dev mode and loads `devUrl` — the Vite
dev server — instead of the embedded frontend. The implementing PR adds a
`custom-protocol` feature to `acter-app` (off by default so `tauri dev` keeps
live-reload); every standalone build, E2E included, must enable it.

## Deliverables

- `e2e/` directory as an npm workspace member: plain WebdriverIO driving the
  WebDriver server embedded in the debug app binary
  (`tauri-plugin-wdio-webdriver`); one app instance per spec on a unique port (see
  amendment above).
- Root script `npm run test:e2e`: builds the frontend, builds the app, runs the
  suite against the real binary.
- **Scenarios (initial suite):**
  1. Smoke: app launches; the edit field is found **by its accessible label**
     ("Command input") — not by CSS selector, so the test fails if the accessible
     name breaks; type a command, submit, assert an h2 with the command text
     appears and the response text appears under it.
  2. Announcer: after submit, the live region contains the response text exactly
     once; the announcer element from before the submit is the **same DOM node**
     (never recreated — the live-region lifecycle rule, machine-enforced).
  3. **axe-core in the real WebView2:** inject axe via the WebDriver session and
     assert zero critical/serious violations on the running app.
- CI: separate `e2e (Windows)` job. Planned as non-blocking for its first two weeks;
  it passed on its first CI run (no drivers, no display trickery needed on the
  hosted runner), so it was flipped to required inside the implementing PR itself.

## Acceptance criteria

1. `npm run test:e2e` passes locally on Windows against the built app.
2. The three scenarios above are implemented and green.
3. CI job runs on the PR itself (non-blocking) and produces readable output on
   failure (screenshots or DOM dumps uploaded as artifacts).

## Out of scope

NVDA/speech automation (impossible in CI; stays manual per DESIGN.md), Linux E2E
(until a Linux target exists), performance testing.

## Amendment (2026-08-30, rides in B9.6): a guard waits for behaviour, not for a sign of it

`menu.spec.ts` was intermittently red on CI — on main as well as on branches — on "opens on
F10 with focus on the first item", expecting focus on `menu-acter` and finding it still on
`command-input`. The menu bar is wired inside an IPC round trip (`shell.platform().then(...)`),
so a spec that presses F10 early presses it at a document with nothing listening.

**The first guard watched a proxy and the flake survived it.** It waited for
`#menu-bar-region` to lose its `hidden` attribute, on the premise that the reveal was "exactly
the moment the listeners are attached" — and in `main.ts` the reveal came *before*
`installMenuBar`. A7 gains an amendment putting those two in the right order, which is correct
on its own terms; this amendment is about the test.

**The rule this establishes for every guard in this suite: wait for the property the file
depends on, not for a sign of it.** The guard presses F10 until focus actually lands in the
bar. It is then indifferent to *how* the race is lost — a late install, a document reloaded
during start-up, an ordering nobody anticipated — where any proxy is only as good as the
belief behind it, and that belief is exactly what a flake falsifies.

A guard of this shape is affordable here because a spec's app instance is its own
(`beforeSession`/`afterSession` above), so pressing a key in a `before` hook cannot leak into
another spec file.

## Ordering

After T1, before A2 — the echo harness from A1 is a sufficient application under
test, and every subsequent UI PR (A2+) then lands against a working E2E ground
line.

## Definition of done

Merged with the CI job visible on the PR; ARCHITECTURE.md test strategy updated in
the same PR (axe moves from "planned" to "implemented via T2 harness").
