# Acter — Design Document

Acter is an accessibility-first terminal for screen reader users. Rust, self-contained,
Windows first (Linux/Mac later), GUI via an HTML frontend.

Status: planning. Decisions are marked **Decided**; everything else is open.

Technical architecture (crate layout, coding paradigm, dependency injection, test
strategy) lives in [ARCHITECTURE.md](ARCHITECTURE.md). Build order and PR plan live
in [ROADMAP.md](ROADMAP.md).

## Vision

A terminal whose default experience is conversational and screen-reader-native, with an
escape hatch to full terminal emulation when a program needs it.

### Non-interactive mode (default)

- An edit field where commands are typed.
- Tab auto-completes; the completion is announced by the screen reader.
- A results buffer sits above the edit field. Each executed command is rendered as a
  heading level 2 with its output below it, so the user gets heading navigation through
  command history.
- Output below a size threshold is read automatically (ARIA live region). Output above
  the threshold is announced as "too big to read" and a beep signals when the command
  finishes.
- Exit codes are known per command (see command boundaries below), so failures can be
  announced distinctly from successes.

### Interactive mode (toggled by a keystroke)

- The edit field becomes a real terminal: keystrokes pass through to the running
  program. Supports nano, ncurses apps, and anything needing full-screen keyboard
  navigation.
- **Decided:** both modes are rendering modes over the *same* live session. Toggling
  modes never restarts the shell or loses state (cwd, environment, running program).
- When a program enters the alternate screen (ESC[?1049h — nano, htop, any ncurses
  app), Acter detects it and can announce that interactive mode is needed, or switch
  automatically (open question: announce vs auto-switch).

## GUI framework

**Decided: Tauri 2.**

Rationale:
- On Windows, Tauri renders through WebView2, which exposes content to NVDA/JAWS/
  Narrator via UIA exactly like a web page. We get the mature web accessibility toolkit:
  aria-live regions, real heading elements, focus management.
- Rust backend with simple IPC to the JS frontend.
- Self-contained small executable; WebView2 ships with Windows 10/11 (bundle the
  bootstrapper as fallback).
- Cross-platform path exists (WebKitGTK on Linux, WKWebView on macOS).

Rejected:
- Native Rust GUI (egui, iced, Slint): AccessKit is still too immature for an
  accessibility-first app.
- Wry directly: Tauri adds windowing/IPC/packaging on top for free.
- Dioxus: workable, but plain HTML/JS gives the most direct control over ARIA and focus.

## Architecture: frontend + pluggable backends

**Decided:** the frontend talks a single uniform protocol; multiple backends supply
sessions (cmd, PowerShell, WSL2, SSH, and future Linux/Mac shells).

**Decided:** the backend concept splits into two independent layers:

### 1. Transport — how bytes reach the shell

- `LocalPty`: ConPTY on Windows (Unix pty later). Covers cmd, PowerShell, and WSL2
  (wsl.exe is just a local process inside the ConPTY).
- `Ssh`: native Rust SSH client (`russh` crate) — we own auth, connection state, and
  reconnection, rather than shelling out to ssh.exe.

### 2. Shell adapter — what shell runs at the far end

- PowerShell, cmd, bash/zsh adapters.
- Knows: session setup (shell-integration injection), quoting rules, completion
  strategy.
- Combinations are free: bash-over-SSH and bash-over-WSL share the bash adapter on
  different transports.

### Frontend protocol (over Tauri IPC)

Conceptually per session:
- input: write bytes / submit command / resize / request completion / toggle mode
- events: output chunks, command boundaries (started / finished + exit code), alt-screen
  entered/left, title change, connection state

### Command boundaries (linchpin of non-interactive mode)

**Decided:** shell integration escape sequences (OSC 133 — same mechanism as Windows
Terminal / VS Code). The shell adapter injects a prompt hook at session start that emits
invisible markers: prompt-start, command-start, command-end + exit code.

- PowerShell / bash: profile snippet injected at session start.
- cmd: PROMPT variable supports $E (escape), so markers ride inside the prompt string.
- Works transparently over SSH — markers travel in the output stream.

This is what closes each response block, measures output size for auto-read vs beep,
and supplies exit codes.

Marker semantics: **A** prompt start, **B** command line accepted, **C** output
begins, **D + exit code** command finished. The prompt only reappears when the
previous command has ended, so D is a deterministic end signal — not a heuristic.

### Reliability model — **Decided**

Three ways D can fail to arrive, each with defined behavior:

1. **Command genuinely still running** (tail -f, REPLs, nested shells). The tracker's
   "running" state is correct. After a configurable **patience window (default 10
   seconds)**, announce once: "command still running; output is accumulating in the
   buffer." Follow mode (Ctrl+Shift+F) is the companion. When D eventually arrives,
   the normal size policy applies to the full output.
2. **Integration silently missing** (user prompt customization clobbers the hook; SSH
   host blocks the snippet). Detected at session start: the injected snippet emits
   markers on the first prompt; if none appear within a grace period the session is
   flagged and announced as **unintegrated**, and every command degrades to case 1
   behavior — patience announcement, manual buffer review, no auto-read. Honest
   degradation instead of wrong guesses.
3. **Forged markers** (a program printing OSC 133 itself). The boundary tracker must
   be robust to nonsensical transitions (e.g. D with no open command is ignored);
   covered by property tests.

### Session shell (persistent) — **Decided**

Commands run inside a persistent real shell via the transport (not spawned
individually), so cd, environment, and aliases persist between commands.

### Completion — **Decided (phased)**

- Phase 1: Acter-native — command history + filesystem paths. Transports provide a
  "list directory" primitive (local fs call locally; via the session over SSH).
- Phase 2: shell-native completion as an adapter capability (PowerShell TabExpansion2,
  bash compgen). Richer, but fiddly to query without disturbing the live session.

## Configuration: profiles — **Decided**

- A **profile** bundles a transport + shell adapter + settings (auto-read threshold,
  beep/announcement preferences, starting directory, ...).
- A **Defaults profile** holds baseline settings; concrete profiles (PowerShell, cmd,
  WSL, an SSH host) inherit from it and override selectively.
- A **session** is an instance of a profile and can override settings further at
  runtime via a per-session configuration screen.
- **Tabs are coming:** each tab is one session. (Non-visual tab navigation UX is an
  open question.)
- **The scripted fake session is a permanent supported session kind** — **Decided**
  (spec A3, decision 9; agreed in conversation 2026-07-19). It is selectable like any
  real shell once the session/profile UI exists, and is the default backend until
  then (the frontend attaches to it automatically at startup). A scriptable session
  stays permanently useful for accessibility experiments, demos, and reproducing NVDA
  findings without a real shell; convergence swaps the default, it does not delete the
  fake.

### Auto-read threshold — **Decided**

- Default: auto-read output up to **25 lines or 2,000 characters, whichever is
  exceeded first**. Dual limit because lines alone mislead (30 short lines of
  git status read fast; 10 lines of minified JSON do not).
- Bias is deliberately generous: an over-long auto-read is silenced with one keypress
  (screen reader speech interrupt), while a too-small threshold forces buffer
  navigation on every medium output.
- Measured on the extracted grid text (trailing whitespace trimmed), never raw PTY
  bytes — escape sequences and prompt redraws would inflate the count.
- Configurable per profile / per session like any other setting.

## Keystroke map

All keybindings are configurable. **Decided:** bindings are a global setting, not
per-profile (muscle memory must not change between sessions).

### Three-layer rule — **Decided**

1. **Acter global commands are Ctrl+Shift+letter.** Identical in every context; never
   passed to the app. Rule in one sentence: "Ctrl+Shift means you're talking to Acter."
2. **Contextual keys keep their native meaning per focus.** Results buffer: standard
   text interaction (Ctrl+C copies selection, Ctrl+A selects all, arrows navigate).
   Edit field: command-line editing; Ctrl+C with a selection copies, without a
   selection interrupts the running command.
3. **Interactive mode passes everything that isn't layer 1 to the app**, including
   plain Ctrl+C (SIGINT via PTY), Alt combos (Meta keys), and Escape.

### Default bindings

- **Ctrl+Shift+E** — toggle interactive / non-interactive mode. **Decided.** (Moved
  from plain Ctrl+E, which collided with readline/nano/emacs end-of-line; terminal
  apps cannot receive Ctrl+Shift combos, so the collision vanishes.)
- **Ctrl+Shift+Space** — pass-through: send the next keystroke literally to the app.
- **Ctrl+Shift+C** — copy the entire last response to the clipboard (no selection
  needed).
- **Ctrl+Shift+R** — re-read the last response without moving focus.
- **Ctrl+Shift+S** — status announcement: profile/session, mode, cwd, last exit code.
- **Ctrl+Shift+L** — clear the results buffer.
- **Tabs:** Ctrl+Shift+T new, Ctrl+Shift+W close, Ctrl+Tab / Ctrl+Shift+Tab
  next/previous (safe unshifted — VT apps cannot see Ctrl+Tab), Ctrl+Shift+1–9 jump
  to tab N.
- **F6** — toggle focus between edit field and results buffer (**Decided**; the one
  non-Ctrl+Shift global, justified by the Windows F6 pane-cycling convention. TUIs
  that use F6, e.g. Midnight Commander, reach it via the pass-through key).
- **Escape** — contextual: in the results buffer, return focus to the edit field; in
  interactive mode, sent to the app.

Deliberately not bound by Acter: speech silencing (screen reader's own key), heading
navigation in the buffer (NVDA browse-mode quick nav over real h2 elements).

## Phasing — **Decided**

Phase 1 builds non-interactive mode only; interactive mode is phase 2. This is safe
because phase 1 bakes in three guardrails that make phase 2 purely additive:

1. **Terminal grid model from day one.** All PTY output runs through a real terminal
   emulation core even in phase 1; the non-interactive text view is derived from the
   grid/scrollback, never from regex-stripping ANSI out of the raw stream. In phase 1
   the grid is a text extractor; in phase 2 the same grid becomes the interactive
   screen. **Decided:** candidate core is `alacritty_terminal` (production engine
   exposed as a library) — moved up from open question because it is a phase 1
   dependency.
2. **Protocol designed for both modes, implemented as a subset.** `resize`,
   `alt-screen-entered/left`, and screen-state events are defined in the protocol now
   even though phase 1 barely uses them. Resize plumbing is nearly free (ConPTY
   requires dimensions at creation).
3. **Alt-screen detection ships in phase 1.** A user can type `nano` into phase 1;
   without detection they get a hung-looking session. Phase 1 response is minimal:
   announce that the program needs interactive mode (not yet available) and how to get
   out (Ctrl+C). The detection hook is the same one phase 2 uses.

Phase 2 then adds only: keyboard routing switch in the frontend (edit field vs
passthrough), a grid renderer, and the interactive-mode screen reading strategy.

## Edit field ownership (non-interactive mode) — **Decided**

**The edit field is 100% local; the terminal never updates it.** The shell sees no
bytes until Enter, when Acter sends the complete line. Mirroring the shell's line
editor (readline) into the field is rejected wholesale: it makes the field a
rendering of remote state (async echo, remotely-owned caret) that no screen reader
can track.

Every line-editing affordance is provided locally instead:
- **History:** up/down navigate Acter's own command history (we hold every submitted
  command), persisted per profile across sessions, deduplicated. Recall is a normal
  announced local text change. Phase 2 nicety: adapters may import existing shell
  history (PSReadLine file, .bash_history) so day-one history isn't empty.
- **Completion:** Acter-native (already Decided): history + filesystem paths.
- **Cursor/editing keys:** native edit-field behavior; the terminal is not involved.

**Invariant:** the terminal updates only the results buffer; the user updates only
the edit field. No third case. (Interactive mode has no edit field, so the question
does not arise there.)

Consequences:
- **Mid-command prompts** ("Y/n" confirmations): prompt text arrives as output of
  the running command (announced promptly via quiescence — see Output pacing); the
  user types the answer in the edit field and presses Enter; because the boundary
  tracker knows a command is running, the line is delivered as stdin to that program
  instead of opening a new command block. Same field, same rule.
- **History exclusion:** a line enters history only if it opened a command block
  (submitted at the prompt). Lines sent while a command runs are program input —
  answers, passwords, REPL input — and are never saved to history.
- **Echo exclusion:** the shell's echo of a submitted line falls between OSC 133
  markers A and C (prompt/echo region); block content is taken from C..D only, so
  the command line is never duplicated under its h2.

## Output pacing: quiescence, patience, follow mode — **Decided**

Silence is a signal: a program that stops printing is either done or waiting for
input. The session actor paces announcements with two timers (via the `Clock` port,
so the whole policy is testable with a fake clock):

1. **Quiescence (default 0.5 s, configurable):** when output pauses for the window,
   the unspoken text accumulated since the last announcement (or since Enter)
   becomes a chunk; the size threshold applies **per chunk** — under it, auto-read;
   over it, announce "N lines arrived, too big to read." This is what gets
   "Password:" or "Continue? y/n" spoken about half a second after they appear,
   mid-command, and narrates long builds phase by phase.
2. **Patience (default 10 s, configurable):** if output flows continuously with no
   quiescent gap for the whole window, announce once: "long command running, output
   accumulating in the buffer."
3. **Command end (D marker):** read the unspoken remainder per policy; announce
   failures distinctly (nonzero exit code). Fast commands finish before quiescence
   ever fires — behavior is a single end-of-command reading, as originally designed.

**Babble guard (proposed default, configurable):** after three consecutive auto-read
chunks within one command (e.g. watch -n1, chatty logs — every burst under threshold
forever), announce "output continues" and go quiet unless follow mode is on.

**Follow mode (Ctrl+Shift+F, default off):** the explicit override — read everything
as it arrives, ignoring thresholds and the babble guard. Its job is intentional
monitoring; quiescence handles the organic cases.

Also decided earlier and unchanged:
- The status announcement (Ctrl+Shift+S) reports when a command is still running.
- The frontend caps rendered lines per block (last N lines) for never-ending output;
  full scrollback is retained backend-side in the terminal grid.

## Open questions

- Alt-screen behavior: announce "interactive mode needed" vs auto-switch (and how to
  announce the switch back).
- Interactive-mode screen reading strategy: how the buffer/grid is exposed to the
  screen reader while a full-screen app runs (review cursor? live row announcements?).
- Non-visual tab/session navigation UX (switch keys, announcing which session is
  active, activity in background tabs — e.g. a long build finishing in another tab).
- SSH auth UX: password prompts, key files, agent support, host key verification —
  all must be fully screen-reader-accessible flows.
- Password / no-echo prompts in non-interactive mode: typing a password into the
  local edit field would display and speak it. Remote no-echo state is unreliable to
  detect through ConPTY; likely a "secure input" toggle masking the field, possibly
  with heuristic detection of password-prompt text as a hint.
