# Acter — Construction Roadmap and Status Board (Phase 1)

Companion to [DESIGN.md](DESIGN.md) (product) and [ARCHITECTURE.md](ARCHITECTURE.md)
(engineering rules). This document owns build order **and execution status**. It is
the answer to "what should we do now?".

## How to use this board — **Decided**

One line per entry. An open entry links to its entry file under [roadmap/](roadmap/),
which holds the finding and its measurements; a spec under [specs/](specs/) replaces that
file when the entry is agreed; the merged PR flips the line to Done and writes nothing
else. The procedure for each of those steps, and the lane rules, is the `roadmap` skill.

- A planning session reads "What is next" and opens one entry file, not the board's
  history.
- **Spec: none yet** means a spec conversation with the user, never code. A spec path
  means implement it, judged against the spec, in one short PR that carries the spec.
- Lanes run in parallel with at most one open PR per lane; order within a lane is strict
  unless the lane says otherwise.
- Checklist results live in the implementing PR's body as checkboxes; findings on the
  unchecked item spawn new entries here.

## What is next

Every entry not marked Done, by lane, in lane order. The first line of each lane is that
lane's next step.

**Lane 1: UI and testing infrastructure**

- 13.5. **Open** — The first Enter after a window opens does not always press the focused control. Entry: [13.5-first-enter-after-window-opens-misses-the-focus.md](roadmap/13.5-first-enter-after-window-opens-misses-the-focus.md). Spec: none yet.
- 14. **Open** — A4, completion path. Entry: [14-a4-completion-path.md](roadmap/14-a4-completion-path.md). Spec: none yet.
- 15. **Open** — A5.3 and onward: iteration entries appear here as NVDA findings arrive. Spec: none yet.
- 49. **Open** — A new menu action compiles and does nothing. Entry: [49-a-new-menu-action-compiles-and-does-nothing.md](roadmap/49-a-new-menu-action-compiles-and-does-nothing.md). Spec: none yet.
- 50. **Open** — The far-end toggle depends on the keyboard layout. Entry: [50-the-far-end-toggle-depends-on-the-keyboard-layout.md](roadmap/50-the-far-end-toggle-depends-on-the-keyboard-layout.md). Spec: none yet.

**Lane 2: domain**

- 22.7. **Open** — B4.7, the results buffer is the pager. Entry: [22.7-b4-7-results-buffer-pager.md](roadmap/22.7-b4-7-results-buffer-pager.md). Spec: none yet.
- 22.8. **Open** — B4.8, a real shell session thinks a command is always running. Entry: [22.8-b4-8-real-shell-session-thinks.md](roadmap/22.8-b4-8-real-shell-session-thinks.md). Spec: none yet.
- 22.9. **Open** — A silent success says nothing at all. Entry: [22.9-silent-success-says-nothing-all.md](roadmap/22.9-silent-success-says-nothing-all.md). Spec: none yet.
- 22.11. **Open** — Caret text from an unclaimed device-query reply still reaches the buffer past the injection point. Entry: [22.11-device-query-reply-reaches-command-line.md](roadmap/22.11-device-query-reply-reaches-command-line.md). Spec: none yet.
- 22.14. **Open** — A marked cmd session grows one empty block after its first command. Entry: [22.14-marked-cmd-session-grows-one-empty.md](roadmap/22.14-marked-cmd-session-grows-one-empty.md). Spec: none yet.
- 23.7. **Open** — A session that is starting says nothing while it starts. Entry: [23.7-session-starting-says-nothing-while-starts.md](roadmap/23.7-session-starting-says-nothing-while-starts.md). Spec: none yet.
- 23.10. **Open** — A cold WSL start outruns the five-second grace period. Entry: [23.10-cold-wsl-start-outruns-five-second.md](roadmap/23.10-cold-wsl-start-outruns-five-second.md). Spec: none yet.
- 23.14. **Open** — In busybox sh, the markers cost the line editor sixteen columns it does not have. Entry: [23.14-busybox-markers-cost-sixteen-columns.md](roadmap/23.14-busybox-markers-cost-sixteen-columns.md). Spec: none yet.
- 23.17. **Open** — A fresh zsh account meets an interactive wizard, and Acter's setup line answers it. Entry: [23.17-fresh-zsh-account-meets-a-wizard.md](roadmap/23.17-fresh-zsh-account-meets-a-wizard.md). Spec: none yet.
- 27.2. **Open** — A connection that waits for a person can be hung up on while it waits. Entry: [27.2-connection-waits-for-person-can-be.md](roadmap/27.2-connection-waits-for-person-can-be.md). Spec: none yet.
- 27.3. **Open** — Reopening the Connect dialog re-reads the last thing the previous attempt said. Entry: [27.3-reopening-connect-dialog-re-reads-last.md](roadmap/27.3-reopening-connect-dialog-re-reads-last.md). Spec: none yet.
- 27.6. **Open** — Windows PowerShell's screen-reader warning is spoken after the prompt the buffer puts it before. Entry: [27.6-powershell-warning-spoken-after-prompt.md](roadmap/27.6-powershell-warning-spoken-after-prompt.md). Spec: none yet.
- 27.7. **Open** — At a bash far end, nothing is read aloud when the session connects. Entry: [27.7-bash-far-end-nothing-read-aloud.md](roadmap/27.7-bash-far-end-nothing-read-aloud.md). Spec: none yet.
- 42. **Open** — A file started anyway goes unmentioned when the far end also has a note. Entry: [42-started-anyway-note-dropped-behind-far-end-note.md](roadmap/42-started-anyway-note-dropped-behind-far-end-note.md). Spec: none yet.
- 43. **Open** — A leftover answer can resolve the next question in the same attempt. Entry: [43-a-stale-answer-resolves-the-next-question.md](roadmap/43-a-stale-answer-resolves-the-next-question.md). Spec: none yet.
- 44. **Open** — A trusted signature whose subject merely contains "Microsoft" is announced as Microsoft's. Entry: [44-any-subject-naming-microsoft-is-called-microsoft.md](roadmap/44-any-subject-naming-microsoft-is-called-microsoft.md). Spec: none yet.
- 45. **Open** — A full reset leaves Acter believing the alternate screen is up. Entry: [45-a-full-reset-leaves-acter-on-the-alternate-screen.md](roadmap/45-a-full-reset-leaves-acter-on-the-alternate-screen.md). Spec: none yet.
- 46. **Open** — A marker inside a synchronized update may be placed before the output ahead of it. Entry: [46-a-marker-inside-a-synchronized-update-may-be-misplaced.md](roadmap/46-a-marker-inside-a-synchronized-update-may-be-misplaced.md). Spec: none yet.
- 47. **Open** — A password is asked for before the server says it takes one. Entry: [47-a-password-is-asked-before-the-server-says-it-takes-one.md](roadmap/47-a-password-is-asked-before-the-server-says-it-takes-one.md). Spec: none yet.
- 48. **Open** — A question nobody will answer parks a thread for good. Entry: [48-an-abandoned-question-parks-a-thread-for-good.md](roadmap/48-an-abandoned-question-parks-a-thread-for-good.md). Spec: none yet.
- 51. **Open** — Four tests and fakes claim more than they check. Entry: [51-four-tests-and-fakes-claim-more-than-they-check.md](roadmap/51-four-tests-and-fakes-claim-more-than-they-check.md). Spec: none yet.

**Lane 3: macOS**

- 33.2. **Open** — The echo of a long submitted line may drop a character per wrapped row. Entry: [33.2-echo-long-submitted-line-may-drop.md](roadmap/33.2-echo-long-submitted-line-may-drop.md). Spec: none yet.
- 34.1. **Open** — M3.5, the macOS help says how to set VoiceOver up. Entry: [34.1-m3-5-macos-help-says-how.md](roadmap/34.1-m3-5-macos-help-says-how.md). Spec: none yet.
- 35. **Open** — M4, bundling, signing and notarising Acter itself. Entry: [35-m4-bundling-signing-notarising-acter-itself.md](roadmap/35-m4-bundling-signing-notarising-acter-itself.md). Spec: none yet.
- 36. **Open** — M5, the far end's line is audible on macOS. Entry: [36-m5-far-end-s-line-audible.md](roadmap/36-m5-far-end-s-line-audible.md). Spec: none yet.
- 36.1. **Open** — Text that arrives as one event is dropped in far-end mode. Entry: [36.1-text-arrives-as-one-event-dropped.md](roadmap/36.1-text-arrives-as-one-event-dropped.md). Spec: none yet.
- 38. **Open** — The set-up dialog reads its whole command aloud on macOS. Entry: [38-set-up-dialog-reads-whole-command.md](roadmap/38-set-up-dialog-reads-whole-command.md). Spec: none yet.
- 39. **Open** — Four smaller things the same run measured, each its own fix. Entry: [39-four-smaller-things-same-run-measured.md](roadmap/39-four-smaller-things-same-run-measured.md). Spec: none yet.

**Lane 5: the look**

- 54. **Open** — V3, the output carries the far end's colours. Entry: [54-the-output-carries-the-far-ends-colours.md](roadmap/54-the-output-carries-the-far-ends-colours.md). Spec: none yet.

**Lane 6: the Windows beta** (last: after lane 5, and possibly after 42 to 51)

- 55. **Open** — The first Windows release is the 0.1 beta. Entry: [55-the-first-windows-release-is-the-0.1-beta.md](roadmap/55-the-first-windows-release-is-the-0.1-beta.md). Spec: none yet.

**Keyboard routing and the changed row**

- 28.12. **Open** — Ctrl+C at an idle prompt says a command failed. Entry: [28.12-ctrl-c-idle-prompt-says-command.md](roadmap/28.12-ctrl-c-idle-prompt-says-command.md). Spec: none yet.
- 29. **Open** — A program that is waiting says so. Entry: [29-program-waiting-says-so.md](roadmap/29-program-waiting-says-so.md). Spec: none yet.

## Status board — lane 1: UI and testing infrastructure

- 1. **Done** — PR 0, scaffold. Spec: [pr0-scaffold.md](specs/pr0-scaffold.md)
- 2. **Done** — A1, static shell. Spec: [a1-static-shell.md](specs/a1-static-shell.md)
- 3. **Done** — A5.1, F6 focuses the most recent command heading. Spec: [a5-buffer-focus-last-heading.md](specs/a5-buffer-focus-last-heading.md)
- 4. **Done** — T1, router integration tests via the Tauri mock runtime. Spec: [t1-router-integration-tests.md](specs/t1-router-integration-tests.md)
- 5. **Done** — T2, end-to-end tests over WebDriver. Spec: [t2-e2e-webdriver.md](specs/t2-e2e-webdriver.md)
- 6. **Done** — A2, protocol. Spec: [a2-protocol.md](specs/a2-protocol.md)
- 7. **Done** — A3, fake session backend. Spec: [a3-fake-session-backend.md](specs/a3-fake-session-backend.md)
- 8. **Done** — A3.1, stopping a running scenario. Spec: [a3.1-interrupt-command.md](specs/a3.1-interrupt-command.md)
- 9. **Done** — A5.2, announcement serialization in the announcer. Spec: [a5.2-announcement-serialization.md](specs/a5.2-announcement-serialization.md)
- 10. **Done** — A6, announcement protocol cleanup. Spec: [a6-announcement-protocol-cleanup.md](specs/a6-announcement-protocol-cleanup.md)
- 11. **Done** — A3.2, the `Ctrl+C` interrupt surface. Spec: [a3.2-ctrl-c-interrupt.md](specs/a3.2-ctrl-c-interrupt.md)
- 12. **Done** — A7, the menu bar and About. Spec: [a7-menu-bar-and-about.md](specs/a7-menu-bar-and-about.md)
- 13. **Done** — A8, the Connect dialog. Spec: [a8-connect-dialog.md](specs/a8-connect-dialog.md)
- 13.1. **Done** — A9, the window says what it is connected to. Spec: [a9-the-window-says-where-you-are.md](specs/a9-the-window-says-where-you-are.md)
- 13.2. **Closed by 13.4 (A10)** — an empty results buffer reads as a bare letter. Entry: [13.2-empty-results-buffer-reads-as-a-bare-letter.md](roadmap/13.2-empty-results-buffer-reads-as-a-bare-letter.md).
- 13.3. **Done** — the far end a connection reached is spoken, every time. Spec: [13.3-the-connection-sentence-is-heard.md](specs/13.3-the-connection-sentence-is-heard.md)
- 13.4. **Done** — A10, the window has two faces, and the connected one is the terminal window. Spec: [a10-the-window-has-two-faces.md](specs/a10-the-window-has-two-faces.md)
- 13.5. **Open** — The first Enter after a window opens does not always press the focused control. Entry: [13.5-first-enter-after-window-opens-misses-the-focus.md](roadmap/13.5-first-enter-after-window-opens-misses-the-focus.md). Spec: none yet.
- 13.6. **Done** — A11, PowerShell is one kind and its editions are its variants. Spec: [a11-powershell-is-one-kind.md](specs/a11-powershell-is-one-kind.md)
- 13.7. **Done** — A13, what a session can tell you, and where that is explained. Spec: [a13-what-a-session-can-tell-you-and-where-that-is-explained.md](specs/a13-what-a-session-can-tell-you-and-where-that-is-explained.md)
- 13.8. **Done** — B9.5 rewrote the connection sentence into A13's register. Spec: [b9.5-the-session-is-set-up-after-it-is-established.md](specs/b9.5-the-session-is-set-up-after-it-is-established.md)
- 13.9. **Done** — ten findings from the user's own pass over the shipped window, fixed in one general PR. Spec: amendments to [a8-connect-dialog.md](specs/a8-connect-dialog.md) (G, H, I), [a10-the-window-has-two-faces.md](specs/a10-the-window-has-two-faces.md), [a13-what-a-session-can-tell-you-and-where-that-is-explained.md](specs/a13-what-a-session-can-tell-you-and-where-that-is-explained.md) and [b9.5-the-session-is-set-up-after-it-is-established.md](specs/b9.5-the-session-is-set-up-after-it-is-established.md) (7 to 11)
- 14. **Open** — A4, completion path. Entry: [14-a4-completion-path.md](roadmap/14-a4-completion-path.md). Spec: none yet.
- 15. **Open** — A5.3 and onward: iteration entries appear here as NVDA findings arrive. Spec: none yet.
- 41. **Done** — The end-to-end test for F10 checks focus before the menu has taken it. Spec: [41-the-startup-focus-leaves-a-moved-focus-alone.md](specs/41-the-startup-focus-leaves-a-moved-focus-alone.md)
- 49. **Open** — A new menu action compiles and does nothing. Entry: [49-a-new-menu-action-compiles-and-does-nothing.md](roadmap/49-a-new-menu-action-compiles-and-does-nothing.md). Spec: none yet.
- 50. **Open** — The far-end toggle depends on the keyboard layout. Entry: [50-the-far-end-toggle-depends-on-the-keyboard-layout.md](roadmap/50-the-far-end-toggle-depends-on-the-keyboard-layout.md). Spec: none yet.
- 52.1. **Done** — A prompt lands in a different place in the buffer from one run to the next. Spec: [52.1-a-prompt-is-reported-when-it-is-drawn.md](specs/52.1-a-prompt-is-reported-when-it-is-drawn.md)

## Status board — lane 2: domain (pure Rust; may start anytime, parallel to lane 1)

- 14. **Done** — B1, foundations. Spec: [b1-foundations.md](specs/b1-foundations.md)
- 15. **Done** — B1.1, pacing policy review fixes. Spec: [b1-foundations.md](specs/b1-foundations.md)
- 16. **Done** — B1.5, session actor and the `Clock` port. Spec: [b1.5-session-actor.md](specs/b1.5-session-actor.md)
- 17. **Done** — B2, the command-block boundary tracker. Spec: [b2-boundary-tracker.md](specs/b2-boundary-tracker.md)
- 18. **Done** — B3, terminal engine. Spec: [b3-terminal-engine.md](specs/b3-terminal-engine.md)
- 19. **Done** — B3.5, the `Transport` port and the scripted byte-level fake. Spec: [b3.5-scripted-transport.md](specs/b3.5-scripted-transport.md)
- 20. **Done** — B3.6, the fake shell and the fake pipe. Spec: [b3.6-fake-shell.md](specs/b3.6-fake-shell.md)
- 21. **Done** — B6, real SessionService. Spec: [b6-session-service.md](specs/b6-session-service.md)
- 22. **Done** — B4, local transport. Spec: [b4-local-transport.md](specs/b4-local-transport.md)
- 22.1. **Done** — B4.1, an interrupt that interrupts. Spec: [b4.1-interrupt-that-interrupts.md](specs/b4.1-interrupt-that-interrupts.md)
- 22.2. **Done** — B4.2, text that scrolled away must not be said twice. Spec: [b4.2-scrolled-text-not-said-twice.md](specs/b4.2-scrolled-text-not-said-twice.md)
- 22.3. **Done** — B4.3, a shell that exited sometimes sends one more read. Spec: [b4.3-a-teardown-read-that-races-the-close.md](specs/b4.3-a-teardown-read-that-races-the-close.md)
- 22.4. **Done** — B4.4, autoread in a session with no boundaries. Spec: [b4.4-autoread-with-no-boundaries.md](specs/b4.4-autoread-with-no-boundaries.md)
- 22.5. **Done** — B4.5, cmd.exe can carry OSC 133 A and B. Spec: [b4.5-cmd-markers-and-unclaimed-replies.md](specs/b4.5-cmd-markers-and-unclaimed-replies.md)
- 22.6. **Done** — B4.6, an interrupt through a proxied shell. Spec: [b4.6-an-interrupt-through-a-proxied-shell.md](specs/b4.6-an-interrupt-through-a-proxied-shell.md)
- 22.7. **Open** — B4.7, the results buffer is the pager. Entry: [22.7-b4-7-results-buffer-pager.md](roadmap/22.7-b4-7-results-buffer-pager.md). Spec: none yet.
- 22.8. **Open** — B4.8, a real shell session thinks a command is always running. Entry: [22.8-b4-8-real-shell-session-thinks.md](roadmap/22.8-b4-8-real-shell-session-thinks.md). Spec: none yet.
- 22.9. **Open** — A silent success says nothing at all. Entry: [22.9-silent-success-says-nothing-all.md](roadmap/22.9-silent-success-says-nothing-all.md). Spec: none yet.
- 22.10. **Closed 2026-08-22** — an interrupt can release a backlog of submitted lines into one block. Entry: [22.10-interrupt-releases-backlog-into-one-block.md](roadmap/22.10-interrupt-releases-backlog-into-one-block.md).
- 22.11. **Open** — Caret text from an unclaimed device-query reply still reaches the buffer past the injection point. Entry: [22.11-device-query-reply-reaches-command-line.md](roadmap/22.11-device-query-reply-reaches-command-line.md). Spec: none yet.
- 22.12. **Done** — B4.9, hearing what you just typed, and a bare Enter that goes nowhere. Spec: [b4.9-hearing-what-you-just-typed.md](specs/b4.9-hearing-what-you-just-typed.md)
- 22.13. **Done** — B4.10, an echo whose last characters arrived as a settlement. Spec: [b4.10-an-echo-that-scrolled-as-it-finished.md](specs/b4.10-an-echo-that-scrolled-as-it-finished.md)
- 22.14. **Open** — A marked cmd session grows one empty block after its first command. Entry: [22.14-marked-cmd-session-grows-one-empty.md](roadmap/22.14-marked-cmd-session-grows-one-empty.md). Spec: none yet.
- 22.15. **Done** — a flood in an unintegrated session took the prompt with it.
- 23. **Split into three entries 2026-08-23** — B5, the shell adapters.
- 23.1. **Done** — B5.1, the `ShellAdapter` port, with cmd behind it. Spec: [b5.1-shell-adapter-port.md](specs/b5.1-shell-adapter-port.md)
- 23.2. **Done** — B5.2, the PowerShell adapter. Spec: [b5.2-powershell-adapter.md](specs/b5.2-powershell-adapter.md)
- 23.3. **Done** — B5.3, the WSL adapter and distro discovery. Spec: [b5.3-wsl-adapter.md](specs/b5.3-wsl-adapter.md)
- 23.4. **Done** — B5.4, the connection catalogue: what can be connected to, and what to say when it cannot. Spec: [b5.4-connection-catalogue.md](specs/b5.4-connection-catalogue.md)
- 23.5. **Done** — Ctrl+D is reachable from the window, and is answered three ways. Spec: [28-far-end-line-mode.md](specs/28-far-end-line-mode.md)
- 23.6. **Done** — the prompt is spoken again in an integrated session. Spec: [b5.6-the-prompt-is-spoken.md](specs/b5.6-the-prompt-is-spoken.md)
- 23.7. **Open** — A session that is starting says nothing while it starts. Entry: [23.7-session-starting-says-nothing-while-starts.md](roadmap/23.7-session-starting-says-nothing-while-starts.md). Spec: none yet.
- 23.8. **Done** — B5.5, the distribution says what shell it runs. Spec: [b5.5-the-distribution-says-what-shell-it-runs.md](specs/b5.5-the-distribution-says-what-shell-it-runs.md)
- 23.9. **Done** — B5.7, what this machine actually has, and who signed it. Spec: [b5.7-what-this-machine-actually-has.md](specs/b5.7-what-this-machine-actually-has.md)
- 23.10. **Open** — A cold WSL start outruns the five-second grace period. Entry: [23.10-cold-wsl-start-outruns-five-second.md](roadmap/23.10-cold-wsl-start-outruns-five-second.md). Spec: none yet.
- 23.11. **Done** — B9.5 removed the ordering all four failures came from. Spec: [b9.5-the-session-is-set-up-after-it-is-established.md](specs/b9.5-the-session-is-set-up-after-it-is-established.md)
- 23.12. **Done** — B9.6 quieted the window Acter talks to itself in. Spec: [b9.6-verdicts-in-sh-and-nothing-read-aloud-at-connect.md](specs/b9.6-verdicts-in-sh-and-nothing-read-aloud-at-connect.md)
- 23.15. **Done** — B9.6 asked it to. Spec: [b9.6-verdicts-in-sh-and-nothing-read-aloud-at-connect.md](specs/b9.6-verdicts-in-sh-and-nothing-read-aloud-at-connect.md)
- 23.16. **Done** — B5.8, zsh is a shell Acter sets up. Spec: [b5.8-zsh-is-a-shell-acter-sets-up.md](specs/b5.8-zsh-is-a-shell-acter-sets-up.md)
- 23.13. **Done** — the connection sentence is sometimes not announced. Spec: [13.3-the-connection-sentence-is-heard.md](specs/13.3-the-connection-sentence-is-heard.md)
- 23.14. **Open** — In busybox sh, the markers cost the line editor sixteen columns it does not have. Entry: [23.14-busybox-markers-cost-sixteen-columns.md](roadmap/23.14-busybox-markers-cost-sixteen-columns.md). Spec: none yet.
- 23.17. **Open** — A fresh zsh account meets an interactive wizard, and Acter's setup line answers it. Entry: [23.17-fresh-zsh-account-meets-a-wizard.md](roadmap/23.17-fresh-zsh-account-meets-a-wizard.md). Spec: none yet.
- 24. **Done** — B6.1, correlation that cannot drift. Spec: [b6.1-correlation-that-cannot-drift.md](specs/b6.1-correlation-that-cannot-drift.md)
- 25. **Done** — B7, sessions that start at runtime. Spec: [b7-sessions-that-start-at-runtime.md](specs/b7-sessions-that-start-at-runtime.md)
- 26. **Done** — the connection manager, and where Acter keeps its settings. Spec: [26-connection-manager.md](specs/26-connection-manager.md)
- 27. **Done** — B9, SSH: a far end that is not on this machine. Spec: [b9-ssh.md](specs/b9-ssh.md)
- 27.2. **Open** — A connection that waits for a person can be hung up on while it waits. Entry: [27.2-connection-waits-for-person-can-be.md](roadmap/27.2-connection-waits-for-person-can-be.md). Spec: none yet.
- 27.3. **Open** — Reopening the Connect dialog re-reads the last thing the previous attempt said. Entry: [27.3-reopening-connect-dialog-re-reads-last.md](roadmap/27.3-reopening-connect-dialog-re-reads-last.md). Spec: none yet.
- 27.4. **Done** — B6.2, what the far end said before its first marker. Spec: [b6.2-what-the-far-end-said-before-its-first-marker.md](specs/b6.2-what-the-far-end-said-before-its-first-marker.md)
- 27.7. **Open** — At a bash far end, nothing is read aloud when the session connects. Entry: [27.7-bash-far-end-nothing-read-aloud.md](roadmap/27.7-bash-far-end-nothing-read-aloud.md). Spec: none yet.
- 27.5. **Done** — the status region says the whole sentence, and the announcement is that same string, from one function. Spec: [a9-the-window-says-where-you-are.md](specs/a9-the-window-says-where-you-are.md)
- 27.6. **Open** — Windows PowerShell's screen-reader warning is spoken after the prompt the buffer puts it before. Entry: [27.6-powershell-warning-spoken-after-prompt.md](roadmap/27.6-powershell-warning-spoken-after-prompt.md). Spec: none yet.
- 27.1. **Done** — B9.5, the session is set up after it is established. Spec: [b9.5-the-session-is-set-up-after-it-is-established.md](specs/b9.5-the-session-is-set-up-after-it-is-established.md)
- 40. **Done** — The terminal engine can emit two rows in the wrong order after the cursor moves up. Spec: [40-a-line-above-keeps-its-place.md](specs/40-a-line-above-keeps-its-place.md)
- 42. **Open** — A file started anyway goes unmentioned when the far end also has a note. Entry: [42-started-anyway-note-dropped-behind-far-end-note.md](roadmap/42-started-anyway-note-dropped-behind-far-end-note.md). Spec: none yet.
- 43. **Open** — A leftover answer can resolve the next question in the same attempt. Entry: [43-a-stale-answer-resolves-the-next-question.md](roadmap/43-a-stale-answer-resolves-the-next-question.md). Spec: none yet.
- 44. **Open** — A trusted signature whose subject merely contains "Microsoft" is announced as Microsoft's. Entry: [44-any-subject-naming-microsoft-is-called-microsoft.md](roadmap/44-any-subject-naming-microsoft-is-called-microsoft.md). Spec: none yet.
- 45. **Open** — A full reset leaves Acter believing the alternate screen is up. Entry: [45-a-full-reset-leaves-acter-on-the-alternate-screen.md](roadmap/45-a-full-reset-leaves-acter-on-the-alternate-screen.md). Spec: none yet.
- 46. **Open** — A marker inside a synchronized update may be placed before the output ahead of it. Entry: [46-a-marker-inside-a-synchronized-update-may-be-misplaced.md](roadmap/46-a-marker-inside-a-synchronized-update-may-be-misplaced.md). Spec: none yet.
- 47. **Open** — A password is asked for before the server says it takes one. Entry: [47-a-password-is-asked-before-the-server-says-it-takes-one.md](roadmap/47-a-password-is-asked-before-the-server-says-it-takes-one.md). Spec: none yet.
- 48. **Open** — A question nobody will answer parks a thread for good. Entry: [48-an-abandoned-question-parks-a-thread-for-good.md](roadmap/48-an-abandoned-question-parks-a-thread-for-good.md). Spec: none yet.
- 51. **Open** — Four tests and fakes claim more than they check. Entry: [51-four-tests-and-fakes-claim-more-than-they-check.md](roadmap/51-four-tests-and-fakes-claim-more-than-they-check.md). Spec: none yet.

## Status board — lane 3: macOS (**opened 2026-08-31**; may run parallel to lanes 1 and 2)

- 32. **Done** — M1, Acter runs on macOS, and SSH is what it offers. Spec: [m1-acter-runs-on-macos.md](specs/m1-acter-runs-on-macos.md)
- 33. **Done** — M2, the Terminal row: the shells this Mac has, and who signed them. Spec: [m2-the-terminal-row.md](specs/m2-the-terminal-row.md)
- 33.1. **Answered, and it was the launch** — VoiceOver was told nothing has keyboard focus because the binary was not in a bundle. Entry: [33.1-voiceover-found-no-focus-in-an-unbundled-build.md](roadmap/33.1-voiceover-found-no-focus-in-an-unbundled-build.md).
- 33.2. **Open** — The echo of a long submitted line may drop a character per wrapped row. Entry: [33.2-echo-long-submitted-line-may-drop.md](roadmap/33.2-echo-long-submitted-line-may-drop.md). Spec: none yet.
- 34. **Done** — M3, the menu bar macOS actually has. Spec: [m3-the-menu-bar-macos-has.md](specs/m3-the-menu-bar-macos-has.md)
- 34.1. **Open** — M3.5, the macOS help says how to set VoiceOver up. Entry: [34.1-m3-5-macos-help-says-how.md](roadmap/34.1-m3-5-macos-help-says-how.md). Spec: none yet.
- 35. **Open** — M4, bundling, signing and notarising Acter itself. Entry: [35-m4-bundling-signing-notarising-acter-itself.md](roadmap/35-m4-bundling-signing-notarising-acter-itself.md). Spec: none yet.
- 36. **Open** — M5, the far end's line is audible on macOS. Entry: [36-m5-far-end-s-line-audible.md](roadmap/36-m5-far-end-s-line-audible.md). Spec: none yet.
- 36.1. **Open** — Text that arrives as one event is dropped in far-end mode. Entry: [36.1-text-arrives-as-one-event-dropped.md](roadmap/36.1-text-arrives-as-one-event-dropped.md). Spec: none yet.
- 37. **Done** — a chord the platform owns is never a keystroke for the far end. Spec: [37-a-chord-the-platform-owns.md](specs/37-a-chord-the-platform-owns.md)
- 38. **Open** — The set-up dialog reads its whole command aloud on macOS. Entry: [38-set-up-dialog-reads-whole-command.md](roadmap/38-set-up-dialog-reads-whole-command.md). Spec: none yet.
- 39. **Open** — Four smaller things the same run measured, each its own fix. Entry: [39-four-smaller-things-same-run-measured.md](roadmap/39-four-smaller-things-same-run-measured.md). Spec: none yet.

## Status board — lane 4: comments (**opened 2026-09-14**; may run parallel to every other lane)

- 1. **Done** — C1, `acter-core` policies. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)
- 2. **Done** — C2, `acter-core` entities and root files, with the regenerated `ui/src/protocol.ts`. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)
- 3. **Done** — C3, `acter-core` ports and controllers. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)
- 4. **Done** — C4, `acter-core` services `connect.rs` and `conversation.rs`. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)
- 5. **Done** — C5, `acter-core` services `session.rs`. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)
- 6. **Done** — C6, `acter-shells`. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)
- 7. **Done** — C7, `acter-transports` sources and examples. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)
- 8. **Done** — C8, `acter-transports` tests. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)
- 9. **Done** — C9, `acter-app` and `acter-term`. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)
- 10. **Done** — C10, frontend sources under `ui/src`. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)
- 11. **Done** — C11, frontend tests under `ui/test` and `e2e`. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)
- 12. **Done** — C12, retire the lane histories under `docs/roadmap/`. Spec: [c-comments-say-what-the-code-cannot.md](specs/c-comments-say-what-the-code-cannot.md)

## Status board — lane 5: the look (**opened 2026-09-23**; may run parallel to every other lane; strict order)

- 52. **Done** — V1, to a sighted person, it looks like a terminal. Spec: [v1-it-looks-like-a-terminal.md](specs/v1-it-looks-like-a-terminal.md)
- 53. **Done** — V2, the prompt, the command and the edit field share one visual line. Spec: [v2-the-prompt-and-the-command-share-a-line.md](specs/v2-the-prompt-and-the-command-share-a-line.md)
- 53.1. **Done** — A heading repeats text that is already on the line above it. Spec: [v2.1-a-heading-that-repeats-the-line-above-is-hidden-from-sight.md](specs/v2.1-a-heading-that-repeats-the-line-above-is-hidden-from-sight.md)
- 53.2. **Done** — A prompt the shell does not mark never shares a line with its command. Spec: [v2.2-a-cmd-prompt-shares-a-line-with-its-command.md](specs/v2.2-a-cmd-prompt-shares-a-line-with-its-command.md)
- 53.3. **Done** — In far-end line mode, the cursor's row is drawn twice. Spec: [v2.3-the-far-end-row-is-drawn-once.md](specs/v2.3-the-far-end-row-is-drawn-once.md)
- 54. **Open** — V3, the output carries the far end's colours. Entry: [54-the-output-carries-the-far-ends-colours.md](roadmap/54-the-output-carries-the-far-ends-colours.md). Spec: none yet.

## Status board — lane 6: the Windows beta (**opened 2026-09-23**; runs last, after lane 5 and possibly after 42 to 51)

- 55. **Open** — The first Windows release is the 0.1 beta. Entry: [55-the-first-windows-release-is-the-0.1-beta.md](roadmap/55-the-first-windows-release-is-the-0.1-beta.md). Spec: none yet.

## Keyboard routing and the changed row — carved out of the phase 2 gate

- 28. **Done** — far-end-line mode: the keyboard goes to the far end, and the row it redraws is what you hear. Spec: [28-far-end-line-mode.md](specs/28-far-end-line-mode.md)
- 28.1. **Done** — Acter wrote the row after the reader had stopped waiting for it. Spec: [28-far-end-line-mode.md](specs/28-far-end-line-mode.md)
- 28.2. **Done** — the content rule never ran, so a `gh` prompt said nothing. Spec: [28-far-end-line-mode.md](specs/28-far-end-line-mode.md)
- 28.3. **Done** — F6 could not reach the results buffer while the far end owned the line. Spec: [28-far-end-line-mode.md](specs/28-far-end-line-mode.md)
- 28.4. **Done** — Tab completion was applied silently. Spec: [28-far-end-line-mode.md](specs/28-far-end-line-mode.md)
- 28.6. **Done** — after a listing Tab the far-end field held the candidate list instead of the line being edited, and stayed wrong. Spec: [28-far-end-line-mode.md](specs/28-far-end-line-mode.md)
- 28.5. **Done** — an anchor taken from a prompt still being drawn headed the next block with the whole row. Spec: [28-far-end-line-mode.md](specs/28-far-end-line-mode.md)
- 28.7. **Done** — you could not tell who had your keys, and the default was backwards. Decided in [DESIGN.md](DESIGN.md) under "Edit field ownership".
- 28.8. **Done** — who gets your keys is remembered per connection. Spec: [26-connection-manager.md](specs/26-connection-manager.md)
- 28.9. **Done** — a trailing space was invisible, so deleting one was silent. Spec: [28-far-end-line-mode.md](specs/28-far-end-line-mode.md)
- 28.10. **Done** — in an integrated session the prompt was announced on every completion redraw. Spec: [28-far-end-line-mode.md](specs/28-far-end-line-mode.md)
- 28.11. **Done** — a failing command was announced again at every empty Enter. Spec: [b6-session-service.md](specs/b6-session-service.md)
- 28.12. **Open** — Ctrl+C at an idle prompt says a command failed. Entry: [28.12-ctrl-c-idle-prompt-says-command.md](roadmap/28.12-ctrl-c-idle-prompt-says-command.md). Spec: none yet.
- 29. **Open** — A program that is waiting says so. Entry: [29-program-waiting-says-so.md](roadmap/29-program-waiting-says-so.md). Spec: none yet.
- 30. **Closed 2026-09-02** — measured, and the answer went into 28. Entry: [30-a-widget-selection-is-visible-to-a-text-diff.md](roadmap/30-a-widget-selection-is-visible-to-a-text-diff.md).

## Convergence (requires B4, B5 and B6 all Done)

Spec: none yet → specify when unblocked. The container swaps the scripted fake
`Transport` for `LocalPty`; an integration test submits a command through the real
service and asserts the exact event sequence. B3.5 and B6 do the work this section
used to carry, so what remains here is one adapter swap. If the lanes were honest,
this PR is boring — that is the success criterion.

Note what this does *not* buy: real ConPTY quirks and real PowerShell prompt
behavior are only exercised by B4, B5 and the transcripts captured there. The
scripted transport reduces the risk they carry; it does not remove it.

## Post-convergence (still phase 1; specify each when reached)

cmd adapter, bash adapter (WSL), profiles + configuration screen, tabs/session
manager UI, keybinding configurability — each its own short spec + PR.

## Phase 2 gate — planning conversation, not code

Interactive mode: grid renderer, keyboard routing, pass-through key, and the
hardest open design question (interactive screen-reading strategy — see DESIGN.md
open questions). Starts as a design session like the ones that produced DESIGN.md,
with a heavyweight model; expect several rounds before the first spec.

**Amended 2026-08-31.** Keyboard routing and the pass-through key have left this gate for
the section above. They turned out to be a different switch from the renderer — who owns
the line, rather than how the screen is presented — and they are the half that pays now,
since they are what makes an `ssh`, a `wsl`, a container or a REPL usable. What is left
here is the screen: a grid renderer, and how a full-screen program is read to someone who
cannot see it. That is still a design conversation, and it is still the hardest question
in the document.

**Amended 2026-09-02, and the gate now has a measured edge rather than an argued one.**
Where this gate begins is **the alternate screen**, and nothing else. A design session
captured nano, `less -X`, `gh pr create` and `bash` on a real pseudoconsole: nano takes the
alternate screen and the other three do not, so everything on this side of that byte —
pagers, inline selection prompts, readline at any far end — belongs to the section above
and needs no renderer, while everything past it belongs here. The three-level taxonomy is
Decided in DESIGN.md under "Where the boundary actually falls".

The merge that this session briefly proposed — folding 28 into this gate on the grounds
that `gh` needed a renderer — was withdrawn once `gh` was measured: two rows change per
arrow, both legible as text, and answering the prompt collapses the list into one line the
far end writes itself. The population here is genuinely full-screen programs, and the
hardest question in the document is still theirs alone.

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
