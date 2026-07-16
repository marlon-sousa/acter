# Spec: PR A1 — static shell with final ARIA semantics

The first accessibility harness PR and the first NVDA-testable artifact. Goal:
validate the semantic skeleton — roles, headings, live region, focus flow — before
any real behavior exists. Backed by a hardcoded echo command; no sessions, no PTY,
no protocol types yet (those are A2/A3).

## Deliverables

### Backend (crates/acter-app)

- Tauri 2 application shell (single window) wired to the `ui/` frontend.
- Dev orchestration: Tauri CLI with `beforeDevCommand`/`beforeBuildCommand` running
  Vite in `ui/`; the CLI ships as `@tauri-apps/cli` pinned in ui/package.json
  (`npm run tauri dev`), with `cargo tauri dev` as an equivalent alternative for
  those who install tauri-cli; README development section updated accordingly.
- One router: `#[tauri::command] echo(text) -> String`, a one-liner delegating to an
  `EchoApi` trait from managed state; `EchoService` implements it (returns the text
  unchanged). Establishes the router → trait → controller pattern from day one, with
  a unit test against the trait.

### Frontend (ui/)

- Vite + vanilla TypeScript project; composition root in `main.ts`.
- Semantic skeleton (final semantics, not placeholders):
  - **Edit field**: single-line text input, accessible label "Command input"
    (visible label or aria-label — decide in implementation, record in A11Y-NOTES).
  - **Results buffer**: a browse-mode-navigable region above the edit field. Each
    submitted command appends: an `<h2>` containing the command line, followed by
    the response text. Plain readable DOM — NOT itself a live region.
  - **Announcer**: a visually-hidden `aria-live="polite"` region, separate from the
    buffer, used for all automatic speech. Node is created once at startup and never
    recreated (live-region lifecycle rule).
- Behavior (via a frontend controller behind view-adapter interfaces, vitest-tested
  with fakes):
  - Enter in the edit field: invoke `echo`, append the h2 + response block to the
    buffer, push the response text to the announcer, clear the edit field.
  - **F6**: toggle focus between edit field and results buffer.
  - **Escape** in the buffer: return focus to the edit field.

### Module conventions

Every TS file declares its role in a header comment; router (`ipc/`) is the only
module importing `@tauri-apps/api`; DOM access only in view adapters.

## Acceptance criteria

Automated (CI):
- Rust: echo service unit test; workspace checks from PR 0 stay green.
- Frontend: `tsc --noEmit` typecheck (Vite/esbuild strips types without checking;
  the typecheck step is where compile-time guarantees live) and vitest — controller
  tested with fake `BackendApi` and fake view adapters: submit flow appends block,
  announces response, clears field; F6/Escape focus logic.

Manual (first NVDA session — findings recorded in docs/A11Y-NOTES.md):
1. On launch, focus is in the edit field and NVDA announces its label.
2. Typing a command and pressing Enter: the response is spoken automatically,
   exactly once (no double-speak from focus movement or DOM churn).
3. NVDA browse mode in the buffer: H / Shift+H jump between command headings; arrow
   keys read the response text under each.
4. F6 announces the newly focused area both ways; Escape from the buffer returns to
   the edit field and NVDA announces it.
5. After ten rapid submissions, announcements neither queue-spam nor go silent
   (live-region sanity under load).

## Out of scope

Protocol enum and specta generation (A2), fake session backend and pacing behavior
(A3), completion (A4), keybinding configurability, tabs, any styling beyond
readable defaults.

## Definition of done

Merged with green CI; the manual checklist above executed with results (pass or
finding) recorded in docs/A11Y-NOTES.md; findings that require changes become A5+
iteration entries in ROADMAP.md.
