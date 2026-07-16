# Acter — Construction Roadmap and Status Board (Phase 1)

Companion to [DESIGN.md](DESIGN.md) (product) and [ARCHITECTURE.md](ARCHITECTURE.md)
(engineering rules). This document owns build order **and execution status**. It is
the answer to "what should we do now?".

## How to use this board — **Decided**

- **Finding the next step:** take the first entry in the status board below that is
  not marked Done, respecting the lane rules. Its "Spec" field tells you what kind
  of work comes next:
  - **Spec: none yet** → the next step is a spec conversation, not code. Write the
    spec with the user, get it agreed in conversation, land it on main, and update
    the entry's Spec field in the same commit. Never code ahead of an agreed spec.
  - **Spec: exists** → implement it in a PR judged against the spec (branch off
    main; short PR; component + trait(s) + tests per CLAUDE.md).
- **Marking done:** the implementing PR flips its own entry to
  "Done (PR #n, date)" as part of the PR. The mark becomes true on main exactly
  when the user merges — no separate bookkeeping commit.
- **Lanes:** lane 1 (UI + testing infrastructure) and lane 2 (domain) may run in
  parallel — at most one open PR per lane, never two in the same lane. Order
  within a lane is strict.
- Manual accessibility checklists and their results live in the implementing PR's
  body as checkboxes; findings are written inline on the unchecked item (NVDA
  version, expected vs observed) and spawn new A5.x entries here.

## Status board — lane 1: UI and testing infrastructure

1. **Done** — PR 0, scaffold. Spec: [pr0-scaffold.md](specs/pr0-scaffold.md).
   Merged as PR #1 (2026-07-16). Workspace, five facade crates, lints, Windows CI.
2. **Done** — A1, static shell. Spec:
   [a1-static-shell.md](specs/a1-static-shell.md). Merged as PR #2 (2026-07-16).
   Tauri app + ARIA skeleton + echo harness; NVDA checklist in the PR body; one
   finding spawned A5.1. Also landed: role-first folder structure, views/
   convention, root npm workspace dev loop.
3. **Next** — A5.1, F6 focuses the most recent command heading. Spec: exists —
   [a5-buffer-focus-last-heading.md](specs/a5-buffer-focus-last-heading.md) →
   implement. Ends with a manual NVDA checklist (results to the PR body).
4. T1, router integration tests via the Tauri mock runtime. Spec: exists —
   [t1-router-integration-tests.md](specs/t1-router-integration-tests.md) →
   implement. Carries the Windows loader-crash investigation gate
   (STATUS_ENTRYPOINT_NOT_FOUND); a documented fallback is an acceptable outcome.
5. T2, end-to-end tests via WebdriverIO Tauri service. Spec: exists —
   [t2-e2e-webdriver.md](specs/t2-e2e-webdriver.md) → implement. Separate
   non-blocking CI job; axe-core inside the real WebView2.
6. A2, protocol. Spec: none yet → specify first. Scope sketch: IPC event/command
   types as entities in acter-core, tauri-specta TypeScript generation, serde
   round-trip tests.
7. A3, fake session backend. Spec: none yet → specify first. Scope sketch:
   scripted SessionApi fake (small output, over-threshold output, failing command,
   slow command, never-ending command) wired into the harness; unlocks the full
   manual matrix (auto-read, too-big + beep, exit-code announcements, pacing).
   The spec conversation must decide: where the beep lives (first real use of the
   Notifier port) and how the frontend renders pacing verdicts (DESIGN.md, Output
   pacing).
8. A4, completion path. Spec: none yet → specify first. Scope sketch: fake
   completion provider, Tab handling in the edit field, completion announcement.
9. A5.2 and onward — iteration entries appear here as NVDA findings arrive.

## Status board — lane 2: domain (pure Rust; may start anytime, parallel to lane 1)

10. B1, foundations. Spec: none yet → specify first. Scope sketch: driven-port
    traits, session entity/state machine, auto-read/pacing policy against a fake
    clock; table tests.
11. B2, boundary. Spec: none yet → specify first. Scope sketch: OSC 133
    recognition + command-block tracker; proptest (never panics on arbitrary
    bytes); golden-fixture format.
12. B3, terminal engine. Spec: none yet → specify first. Scope sketch: acter-term
    wrapping alacritty_terminal behind TerminalEngine; text extraction +
    alt-screen detection tests.
13. B4, local transport. Spec: none yet → specify first. Scope sketch: LocalPty on
    ConPTY + blocking-reader thread; real-shell integration test in a separate CI
    job.
14. B5, PowerShell adapter. Spec: none yet → specify first. Scope sketch: OSC 133
    injection snippet; record the first golden transcripts as fixtures.
15. B6, real SessionService. Spec: none yet → specify first. Scope sketch:
    implements SessionApi; tested against fake driven ports.

## Convergence (requires A3 and B6 both Done)

Spec: none yet → specify when unblocked. The container swaps the fake SessionApi
for the real SessionService; an integration test submits a command through the real
service against fake transports and asserts the exact event sequence. If both lanes
were honest, this PR is boring — that is the success criterion.

## Post-convergence (still phase 1; specify each when reached)

cmd adapter, bash adapter (WSL), profiles + configuration screen, tabs/session
manager UI, keybinding configurability — each its own short spec + PR.

## Phase 2 gate — planning conversation, not code

Interactive mode: grid renderer, keyboard routing, pass-through key, and the
hardest open design question (interactive screen-reading strategy — see DESIGN.md
open questions). Starts as a design session like the ones that produced DESIGN.md,
with a heavyweight model; expect several rounds before the first spec.

## Principles — **Decided**

- PRs are short. Each PR delivers one coherent unit: component + its trait(s) + unit
  tests. Nothing lands without its tests.
- **UI-first via fake backend.** The frontend depends on the `SessionApi` driving
  port; the first implementation is a scripted fake. Manual NVDA testing — the
  slowest feedback loop in the project — starts immediately and runs continuously.
  Because fake and real service implement the same trait and protocol, manually
  validated UI behavior carries over unchanged at convergence.
- **Testing ground line before UI behavior:** T1/T2 land before A2, so every
  subsequent UI PR lands against router integration tests and E2E checks.
