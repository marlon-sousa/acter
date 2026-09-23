# 41 — The end-to-end test for F10 checks focus before the menu has taken it.

Lane: lane-1-ui. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**The end-to-end test for F10 checks focus before the menu has taken it.** Spec: none yet
→ specify first.

Found on 2026-09-23 by CI on PR #66 (lane 4, C3), in the `e2e (Windows)` job. That PR
changed only comments in `acter-core`. `e2e/test/specs/menu.spec.ts`, test "the menu bar
opens on F10 with focus on the first item", failed at line 86 with
`Expected: "menu-acter"`, `Received: "command-input"`. The rerun of the same commit passed,
and main had been green in every run before.

The test presses F10 and reads the focused element once, immediately. Nothing in the test
waits for focus to move. The `before` hook in the same file uses `waitUntil` with a
30-second timeout for F10 to reach the menu bar, but the test itself does not.
