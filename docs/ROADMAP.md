# Acter — Construction Roadmap (Phase 1)

Companion to [DESIGN.md](DESIGN.md) (product) and [ARCHITECTURE.md](ARCHITECTURE.md)
(engineering rules). This document owns build order.

## Principles — **Decided**

- PRs are short. Each PR delivers one coherent unit: component + its trait(s) + unit
  tests. Nothing lands without its tests.
- **UI-first via fake backend.** The frontend depends on the `SessionApi` driving
  port; the first implementation is a scripted fake. Manual NVDA testing — the
  slowest feedback loop in the project — starts immediately and runs continuously.
  Because fake and real service implement the same trait and protocol, manually
  validated UI behavior carries over unchanged at convergence.
- Two parallel tracks after the scaffold. Order within a track is strict; PRs across
  tracks interleave freely.
- Manual accessibility checklists and their results live in the implementing PR's
  body as checkboxes; findings are written inline on the unchecked item (NVDA
  version, expected vs observed) and spawn A5+ iteration entries here.

## PR 0 — scaffold

Workspace, five crates with facade lib.rs files, `#![warn(unreachable_pub)]`,
clippy/rustfmt config, CI on a Windows runner. No logic.

## Track A — accessibility harness (priority; manual NVDA testing lives here)

- **A1 — static shell.** Tauri app + frontend skeleton: edit field, results buffer
  with final ARIA semantics (roles, live region, real h2 elements), hardcoded echo
  command. First NVDA session: validate the semantic skeleton before any behavior.
- **A2 — protocol.** IPC event/command types in acter-core + tauri-specta TypeScript
  generation; serde round-trip tests.
- **A3 — fake session backend.** Scripted `SessionApi` fake: small output, output
  over the auto-read threshold, failing command (exit code), slow command. Wired
  into the harness. Unlocks the full manual matrix: auto-read, "too big" + beep,
  exit-code announcement, F6/Escape focus flow, heading navigation.
- **A4 — completion path.** Fake completion provider + Tab handling in the edit
  field + screen-reader announcement of the completion.
- **A5…An — iteration.** PRs driven by manual NVDA reports. Explicitly budgeted;
  this is where screen-reader behavior gets tuned, over several rounds.

## Track T — testing infrastructure (right after A1; ground line for UI testing)

- **T1 — router integration tests via the Tauri mock runtime**
  ([spec](specs/t1-router-integration-tests.md)). Every router tested through the
  real invoke pipeline in plain cargo test. Carries a known Windows loader-crash
  investigation gate with an explicit fallback path.
- **T2 — end-to-end tests via WebdriverIO Tauri service**
  ([spec](specs/t2-e2e-webdriver.md)). Real app, real WebView2, real accessibility
  tree: smoke scenario by accessible name, live-region lifecycle assertions, and
  axe-core inside the running engine. Separate non-blocking CI job until stable.

Ordering: T1 then T2, both before A2 — every subsequent UI PR lands against this
ground line.

## Track B — domain (parallel; automated tests only)

- **B1 — foundations.** Driven-port traits, session entity/state machine, auto-read
  policy. Table tests.
- **B2 — boundary.** OSC 133 recognition + command-block tracker. Proptest (parser
  never panics on arbitrary bytes) + golden-fixture format.
- **B3 — terminal engine.** acter-term wrapping alacritty_terminal behind
  `TerminalEngine`; text extraction + alt-screen detection tests.
- **B4 — local transport.** `LocalPty` on ConPTY + blocking-reader thread.
  Real-shell integration test in a separate CI job.
- **B5 — PowerShell adapter.** OSC 133 injection snippet; record first golden
  transcripts as fixtures.
- **B6 — real SessionService.** Implements `SessionApi`; tested against fake driven
  ports.

## Convergence

Composition root swaps fake `SessionApi` for the real `SessionService`. If both
tracks were honest, this PR is boring — that is the success criterion.

## Post-convergence (still phase 1)

Each its own short PR: cmd adapter, bash adapter (WSL), profiles + configuration
screen, tabs/session manager UI, keybinding configurability.

## Phase 2 (not planned in detail yet)

Interactive mode: grid renderer, keyboard routing, pass-through key, interactive
screen-reading strategy (open question in DESIGN.md), SSH transport UX.
