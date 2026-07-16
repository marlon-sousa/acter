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

## Deliverables

- `e2e/` directory as an npm workspace member: WebdriverIO + `@wdio/tauri-service`
  configured with the **embedded provider** (fallback to tauri-driver + Edge
  WebDriver documented in the config if embedded proves unreliable).
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
- CI: separate `e2e (Windows)` job. Non-blocking (continue-on-error) for its first
  two weeks of life; flipped to required once stable — the flip is a one-line PR
  referencing this spec.

## Acceptance criteria

1. `npm run test:e2e` passes locally on Windows against the built app.
2. The three scenarios above are implemented and green.
3. CI job runs on the PR itself (non-blocking) and produces readable output on
   failure (screenshots or DOM dumps uploaded as artifacts).

## Out of scope

NVDA/speech automation (impossible in CI; stays manual per DESIGN.md), Linux E2E
(until a Linux target exists), performance testing.

## Ordering

After T1, before A2 — the echo harness from A1 is a sufficient application under
test, and every subsequent UI PR (A2+) then lands against a working E2E ground
line.

## Definition of done

Merged with the CI job visible on the PR; ARCHITECTURE.md test strategy updated in
the same PR (axe moves from "planned" to "implemented via T2 harness").
