# 41 — the startup focus hold leaves a focus the user moved alone

Roadmap entry 41, lane 1. The fix rides in PR #71, as the user asked on 2026-09-23.

## What was observed

`e2e/test/specs/menu.spec.ts`, test "the menu bar opens on F10 with focus on the first
item", failed with `Expected: "menu-acter"`, `Received: "command-input"`:

- on PR #66, which changed only comments in `acter-core`; the rerun passed;
- on PR #71, on commit `9f9d832`, which removed only two CSS comments. The same job had
  passed on the commit before it, and the rerun passed.

It is always the first test in the file that fails.

The entry recorded two suspected causes. First, the test reads focus once, immediately after
F10. Second, `beforeEach` does not close the bar that the `before` hook left open.

## The cause

Neither. F10 is handled synchronously: `press` dispatches the key on
`document.activeElement`, and the document's F10 listener focuses the first bar item in the
same turn. `beforeEach` focuses `command-input`, so focus is not on a bar item and F10
enters the bar rather than leaving it.

The cause is `WindowChrome.showTerminal`. On the first connection it defers placing focus by
`STARTUP_HOLD_MS` (400 ms), and when the timer fires it focuses the command line
unconditionally. Every e2e spec file starts a fresh app. If the hooks and the first test's F10
all finish within those 400 ms of the connection, the timer then takes focus back out of the
menu. That is exactly `Received: "command-input"`. A user who presses F10 or Tab straight
after connecting loses their place the same way.

## Decisions

1. **When the hold ends, focus moves only if it is still stranded,** by the same test
   `showTerminal` applies when it is called: nothing focused, the body, or an element inside
   something that just went away. The test lives in one private method used by both places.
2. **`menu.spec.ts` does not change.** With the cause gone, reading focus immediately after
   F10 is correct, because the handler is synchronous. A wait in the test would have hidden
   the bug instead of fixing it.

## Files touched

- `ui/src/adapters/window_chrome.ts`
- `ui/test/adapters/window_chrome.test.ts`

## Acceptance criteria

1. A unit test: with a startup hold, the terminal is shown, focus is moved to another element
   during the hold, and after the hold focus is still on that element. Measured: it fails on
   the old code with focus on `command-input`, the CI error, and passes on the new code.
2. The existing startup-hold tests still pass: the first placement is deferred, and later
   ones are immediate.
3. `npm run test:ui` and `npm run test:e2e` pass.
