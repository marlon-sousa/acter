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

**Amended 2026-08-22 (B4.5): a shell may be able to mark only A and B, and cmd is one.**
`PROMPT` is evaluated when the prompt is drawn and `cmd.exe` has no post-execution hook, so
"output begins" and "the command ended with this code" have nowhere to come from. Such a
shell **declares** what it can mark, and two rules change for it; nothing changes for a
shell that marks the full cycle.

- **C is synthesized at the end of the echoed line, and that is evidence rather than a
  guess.** In a line-oriented shell the echo is the one line the shell read, and Acter
  knows the exact text it submitted. The boundary tracker ends the command-line region when
  anything arrives that is not a further append to the row the prompt was drawn on, and
  opens the block there. Text on a *new* row therefore ends the region and is labelled
  output rather than being excluded as an echo, which is what keeps the exclusion below
  from ever hiding a line.
- **The block closes on B, not on A, and the returning prompt is block content.** There is
  no exit code, so nothing is announced when such a command ends; the prompt coming back is
  the only ending a listener gets. Closing on A would leave it belonging to no block, and
  excluding it as prompt region would end every command in silence. It is the one case
  where the prompt region is content, and it exists because the alternative is a session
  that says nothing when it finishes.

D never arrives in such a shell, so a verdict stays unavailable and every command reports
having ended without one — which is what an unintegrated session already produced.

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
   behavior — patience announcement, manual buffer review, no exit code and no verdict.
   Honest degradation instead of wrong guesses.

   **Amended 2026-08-22 (B4.4): such a session is still read aloud.** This rule used to
   end "no auto-read", and that made a real shell silent — everything rendered to the
   buffer, none of it spoken, so a user had to go and review the buffer to learn what any
   command did. The honest-degradation argument was written when the alternative was
   *guessing*: a prompt matched by its shape, output attributed to a command by
   inference. Repeating text that genuinely arrived is not a guess. It is the same
   category as autoread everywhere else, and it is the distinction B4.1 turned on when
   `command stopped` was removed — a claim about something Acter could not observe is a
   guess, and text that was received is evidence. What degrades is what the session can
   *conclude*, not whether it speaks.
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

## Application menu and connecting — **Decided**

Agreed in conversation 2026-08-23. This is the first user-facing surface over the profile
machinery above, and it is what turns Acter from "a session chosen by an environment
variable at launch" into an application a user drives.

### The menu bar is native — **Decided**

A real window menu, built with Tauri's `MenuBuilder` and attached to the main window.
Underneath, Tauri's `muda` builds a Win32 `HMENU`, which Windows exposes to a screen
reader with no work from us: NVDA announces a menu bar, Alt reaches it, arrows navigate it,
and there is no browse-mode/focus-mode switch anywhere in the interaction.

**An in-page ARIA `menubar` was rejected**, and not narrowly: it would put a mode switch in
front of the one control every user of this product must be able to reach, in exchange for
E2E testability. The cost is real and accepted — WebDriver drives the webview only, so a
native menu is covered by Rust tests over its definition plus a manual NVDA pass, which is
the same trade the product already makes for anything below the webview.

Two menus, and no more until something earns one:

- **Acter** — Connect…, Exit.
- **About** — About Acter. A top-level menu holding one item rather than a top-level item
  that acts, because a menu bar entry that fires instead of opening is a surprise to
  anyone navigating by arrow keys.

### Dialogs are HTML modals in the window — **Decided**

The menu is native; what it opens is not. A `<dialog>` in the main window reuses the focus
discipline, the announcer and the test suites the product already has, its text is
browse-mode readable and copyable, and both vitest and WebDriver can drive it. Native
message boxes would add a dependency and a surface neither suite can reach.

### Connecting replaces the session — **Decided**

Phase 1 runs **one session at a time**. Connecting tears down the outgoing shell, clears
the buffer to a clean boundary, and the change is announced — the listener is told which far
end they are now on, because the alternative is a window that looks the same and answers
differently. Tabs remain the later answer, with each tab one session, as above.

**There is deliberately no "something is still running" confirmation.** It would have to
ask the session whether a command is outstanding, and that answer is currently stuck at
"yes" for the whole life of a real shell session (roadmap 22.8). A confirmation that fires
every single time teaches the user to dismiss it, which is worse than not having one.
It becomes buildable when 22.8 lands, and is reconsidered then rather than guessed at now.

### The connectable list comes from the backend, and it is flat — **Decided**

The frontend hardcodes nothing: installed WSL distributions are discovered at runtime, and
the scripted profiles exist only in debug builds. So the backend answers what can be
connected to, and the dialog renders exactly that.

**Flat, one entry per connectable thing** — "WSL: Ubuntu" is an entry, not a WSL entry with
a nested distro choice. A conditional second control that appears only for one option is
harder to navigate non-visually than a longer list of equals, and a list is what arrow keys
and first-letter navigation are already good at.

The scripted fake sessions appear in this list in debug builds, which is what the profiles
section above promised: the fake is a permanent, selectable session kind rather than a
launch-time environment variable.

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

   **The interrupt belongs to the edit field and to nowhere else — Decided.** This was
   already implied by naming it only there, and A3.2 showed the implication needs saying
   out loud: in the results buffer with nothing selected, Ctrl+C does not interrupt and
   Acter does nothing with it at all. The reason is not symmetry, it is that the key is
   not ours to take. Where the buffer lives, a screen reader is in its own browse mode
   and Ctrl+C is *its* copy command: NVDA answers "no selection" itself and the keystroke
   never reaches the application. A binding that cannot be pressed is worse than no
   binding, because it reads as an interrupt the user can rely on.

   **Consequence for implementers: the session hears a keystroke only while the edit
   field has focus and holds no selection.** Screen-reader mode is deliberately not part
   of that test. Browse versus focus mode is the reader's own state, invisible to a web
   frontend and carried by no event, and it does not need detecting: browse mode does not
   deliver the key in the first place. So the rule has two owners, and only the second
   half is ours to enforce. Do not attempt to detect the reader's mode.
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
  user types the answer in the edit field and presses Enter; the line is delivered as
  stdin to that program instead of opening a new command block. Same field, same rule.
- **What decides that — amended 2026-08-22, and the amendment reverses the original
  test.** This bullet used to read "because the boundary tracker knows a command is
  running". That test was wrong in one direction, and the direction matters: a shell
  proxying further commands is also a command running. So inside `docker run -it`, an
  `ssh`, a `wsl`, a `sudo su` or a REPL, every line the user typed was stdin by the rule,
  and a whole nested session collapsed into one block under one heading — no per-command
  boundaries, no way to jump command to command, nothing to review but a straight read.
  The rule and that defect were the same rule.

  The test is now **positive evidence that the far end treated the line as a command
  line**: **the far end echoed it.** Matching is against the accumulated recent output
  rather than a single chunk, because a pseudoconsole hands over whatever it has and a
  command line wide enough to wrap may arrive across two line items; the window is
  bounded by the longest line still waiting, and the match must begin at a word boundary.
  A password never echoes, and a bare Enter into a running program produces no matching
  echo, so both stay stdin.

  **A second part was specified and then withdrawn before it shipped**, and the reason is
  worth keeping. It required that the **prompt anchor** — the text on the cursor row at
  the moment of submission — be drawn again once the line's output ended, which is what
  tells a nested shell (whose prompt comes back) from a `y` at a `[y/N]` (whose does
  not). Tracing it against a plain session showed it regresses the commonest thing anyone
  does: after `cd projects` the prompt is a different string, it never reappears, and the
  next `dir` would be filed as stdin with no block and no heading — on every `cd`, to
  protect a case that arises occasionally. There is no third option that keeps both: once
  a command goes quiet leaving a new row on screen, "the prompt is back" and "the program
  is asking a question" are indistinguishable without markers, and a changed prompt has no
  reappearance to offer. The anchor is deferred to the entry that makes it pay, when an
  integrated session has nested shells that would otherwise get no boundaries at all.

  Until then a `y` echoes and therefore opens a block, which is what an unintegrated
  session already did — a fix deferred, not a defect introduced. Echo proves the line was
  *read as a line*; it does not say what the reader then did with it.

  Where OSC 133 markers reach, the question does not arise — a `B` after a submission is
  the far end saying the line is a command. The evidence test is the answer for far ends
  marker injection can never reach, which is every shell past the one Acter spawned.
- **No echo means no block**, and that is the direction this rule is permitted to fail
  in. A line nobody echoed is stdin, and stays out of history. Guessing toward "command"
  would put an answer — possibly a secret — into a heading and into history; guessing
  toward "stdin" gives a coarser buffer with nothing lost, nothing duplicated and order
  preserved. The buffer must not mangle; structure may be imperfect.
- **The block opens where the echo arrives, not where Enter was pressed**, and nothing
  is opened retroactively. Text that arrives before any echo belongs to no command the
  user submitted — it is the shell's own prompt, or its banner — and gets a block with no
  heading, which is what a session's first prompt produces.

  **"No heading" means no heading element at all — clarified 2026-08-23 (B4.9), after a
  listener met the other reading.** The frontend had been rendering such a block as an
  *empty* level 2 heading, which is a different thing and a worse one: heading navigation
  lands on it and it announces nothing, so the one navigation the buffer exists to offer
  runs into a dead end. The text still reaches the buffer and is still read aloud; what
  it does not get is a heading with nothing in it. Headings are commands, and text no
  command accounts for is text.

  **The echo itself is held and dropped — amended 2026-08-23 (B4.9), reversing the half
  of this bullet that let it through.** It used to stay in the block that was open when
  the far end wrote it, on the grounds that removing it would mean holding every row back
  until it was complete, delaying speech and stranding text when a far end goes quiet
  mid-row. That objection is sound about a rule that holds every row, and it is not this
  rule. A listener heard what it cost: every command after the first read the user's own
  typing back at them before answering it, and inside a container every line typed did.

  **What replaces it is positional and exact, not a comparison.** At the instant Enter is
  pressed the far end has drawn its prompt and its cursor is on that row, and the only
  thing that reaches the far end is what Acter wrote — so everything appended to *that row
  after that instant* is the echo. Only that row is held, bounded by the longest line
  still waiting, and anything past either bound is published at once. Comparing a row's
  text against the heading was the other candidate and is rejected: running `dir` twice
  makes `dir` both a heading and a plausible output row, and equality would hide output,
  which is the cardinal defect. Position cannot.

  It needs no markers, which is the point — it reaches inside a container, an `ssh`, a
  `wsl` and a REPL, where OSC 133 cannot. **The prompt survives it** and is still the last
  thing announced after every command, because the prompt is forwarded before Enter is
  pressed and only what lands after it is held.

  Two shapes stay uncovered and fail toward an echo spoken once, never toward hidden
  text: a far end that ends the row before the echo starts, and the second of two lines
  typed ahead, whose echo lands on a row the far end chooses only after finishing the
  first.
- **An empty submission is sent and opens no block — Decided 2026-08-23 (B4.9).** A bare
  Enter is a re-orient gesture rather than a command: it goes to the far end like any
  other line, and the prompt the shell draws in reply is the answer to it. It matches no
  echo, so by the rule above it opens no block, and it is carried in no correlation queue
  — an id that can never be claimed would be taken by a later block, which is the drift
  correlation exists to prevent. It is also ordinary input to a running program: a REPL, a
  "press Enter to continue". The frontend used to discard it, so nothing was written, the
  shell never redrew its prompt, and a user asking where they were heard silence.
- **What this fixed, and it is why the rule changed at all.** A submission the far end
  never read — a `docker run -t` holding a tty it never attaches stdin to — used to open
  a block immediately, so the user got a heading with nothing under it, and a backlog
  released later filled whichever block happened to be open last. No echo now means no
  block, so the headings appear when the lines actually run.
- **History exclusion:** a line enters history only if it opened a command block. Lines
  delivered as stdin are program input — answers, passwords, REPL input — and are never
  saved to history. The wording is untouched by the amendment above: it was always
  phrased in terms of opening a block, and only what opens one has changed.
- **Echo exclusion:** the shell's echo of a submitted line falls between OSC 133
  markers A and C (prompt/echo region); block content is taken from C..D only, so
  the command line is never duplicated under its h2. **Where markers do not reach** —
  every far end past the injection point, and any unintegrated session — that exclusion
  cannot fire, and the echo matcher above suppresses the duplicate instead. Two halves of
  one fact, chosen by whether the region is marked.

  **Amended 2026-08-22 (B4.5).** In a shell that marks only A and B the C is Acter's own,
  synthesized at the end of the echoed line, so the exclusion fires on a region Acter
  closed rather than one the shell delimited. The rule that keeps that safe is positional:
  only text appended to the row the prompt was drawn on stays in the excluded region.
  Anything on a new row ends the region and becomes output — a block that should not
  have opened is extra structure, and a line that is never spoken is the cardinal defect.
  The prompt region is content in such a shell, which is the one exception to "block
  content is C..D" and is stated with its reason under command boundaries above.

  **Amended 2026-08-23 (B4.9).** The unmarked half is positional too, and by the same
  test one layer out: everything appended to the row the submission is pending on is the
  echo. The two halves are now one rule chosen by whether a region exists to express it,
  which is what makes the seam between a marked shell and the container it launched
  disappear.

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

**Buffer and speech are separate paths — Decided.** The buffer loads whenever content
arrives; only speech is subject to policy. Everything above — quiescence, the size
threshold, patience, the babble guard, follow mode — decides what is *said*, never what
the user can *find*. Going quiet always means unannounced, never withheld: a user who
stops being read to must still be able to review the command's output as it happens.

Two consequences. First, the two paths run at different cadences: the buffer is fed on
the actor's short coalescing tick (tens of milliseconds — see ARCHITECTURE, output
coalescing), while speech decisions are made on the pacing windows, so one spoken unit
spans many rendered ones and the two can no longer travel as a single event. Second,
nothing accumulates backend-side waiting for permission to be shown, which is what a
gapless flood (`yes`, a busy `tail -f`) would otherwise force: no quiescent gap ever
occurs, so under a single coupled path the buffer would stay empty for the whole run.

The invariant that binds the two: **never announce text that is not already in the
buffer.** A user who hears something and presses F6 to review it must find it there.

How the split is expressed in the protocol — announcements carrying their own text
versus addressing a span of already-rendered output — is B1.5's to settle, and it is
the same seam as the announcement-channel open question below.

**Output is a stream of identified lines, and a rewrite is a revision — Decided.**
(Agreed in conversation 2026-08-17, while specifying B3.) A terminal's output is not an
append-only text stream, however much it looks like one: programs rewrite what they have
already written. A `\r` progress bar, a spinner, `cargo`'s status line, `docker pull`'s
stack of per-layer bars — all of them repaint lines in place on the primary screen, with
no alternate screen involved.

That cannot be deferred to the frontend, because it changes what the backend must emit.
The engine therefore emits **lines with identity**: an opaque id minted when a line is
first produced, and each item saying whether its text was *appended* to that line,
*rewrote* it, or *settled* it as final.

The alternative — emitting the line again as fresh text every time it changes — was
considered and rejected. It fills a review buffer with hundreds of copies of a spinner,
and for a user who navigates by reading rather than glancing, a buffer full of
near-identical lines is not a cosmetic problem: it is the thing they have to read
through to find the output that mattered.

Identity is session-global and outlives the command block that produced it, so the
frontend can always find a line. The *right to revise* it does not: closing a block
freezes its lines, and a later rewrite of the same screen rows produces new lines
instead. A review buffer whose history changes behind the reader is worse than a
duplicate.

This is what makes the separate paths above concrete rather than merely a cadence
difference. The **buffer** applies every revision, so it always shows current state.
**Speech** takes appended text as it always has, ignores rewrites as churn, and takes
the settled text as a line's final word — so a spinner is never read mid-spin, while its
result still is. The binding invariant is unaffected: settled and appended text is in
the buffer before it is ever announced.

Consumer-side consequences are recorded against B6: the unspoken-text accumulator is
currently flat and append-only (it drops text once a too-big verdict settles), so making
speech correct under rewrites means keying it by line; the protocol needs a line-aware
wire format; and the frontend needs a map from id to rendered node.

Also decided earlier and unchanged:
- The status announcement (Ctrl+Shift+S) reports when a command is still running.
- The frontend caps rendered lines per block (last N lines) for never-ending output;
  full scrollback is retained backend-side in the terminal grid.

## Open questions

- Browse-cursor stability under in-place updates (raised 2026-08-17 with the
  identified-lines decision above, and to be answered by observation, not argument):
  when a line already rendered in the buffer is *mutated* while the user is sitting in
  browse mode reading, does NVDA's review cursor hold its position, or move? This is not
  about announcement — the buffer is not a live region, so a mutation is silent by
  construction, and whether an update is *spoken* is our own policy choice. It is about
  whether a progress line repainting several times a second yanks a user who pressed F6
  to review an earlier command. If it does, the answer is likely to suspend updates while
  the buffer holds focus. Probeable today with a static page and the screen-readers
  bridge, ahead of any frontend work.
- Alt-screen behavior: announce "interactive mode needed" vs auto-switch (and how to
  announce the switch back).
- **The two-mode model has no room for sending a single keystroke while staying
  conversational** (raised 2026-08-22 by the `git diff` case). The Vision section offers
  non-interactive mode, which has an edit field and submits whole lines on Enter, and
  interactive mode, which "has no edit field" and passes everything through. A pager wants
  neither: the user needs to send one `space` to see the next page and then go straight
  back to reading the buffer, and switching the entire UI into a full-screen rendering to
  send one byte is the wrong trade. The same shape covers a `[y/N]` that expects a bare
  keypress, and `q` to quit a pager.
  If a keystroke can be sent without leaving the conversational view, the buffer needs a
  rule for what it does to structure, and the answer should be **nothing**: a heading
  exists to be navigated to and answers "what did I run?", so a keystroke driving a running
  program earns neither a heading nor an echo line, and whatever it produces appends to the
  block already open. Twenty presses of `space` through a diff must not produce twenty
  headings called "space". Note this is a decision Acter can make *without a heuristic* —
  it knows whether a keystroke arrived as an Enter-submission or as a pass-through, so the
  category is a fact rather than an inference, and nothing here should be timing-dependent.

- **A program can need keystrokes without ever entering the alternate screen, so
  alt-screen detection is not sufficient to know that interactive mode is needed** (raised
  2026-08-22 by the `git diff` case, and measured rather than argued). The Vision section
  above makes `ESC[?1049h` the signal for "a program needs interactive mode". Git's pager
  defeats it: git sets `LESS=FRX` when `LESS` is unset, and `-X` suppresses termcap
  initialisation, so `less` pages without ever touching the alternate screen. Measured
  against a real `cmd.exe` on a real pseudoconsole: 8 KB of `git diff` output arrived and
  neither `ESC[?1049h` nor `ESC[?47h` appeared anywhere in the stream. The user then
  presses space to see the next page, and Acter has no signal at all — no event, no
  announcement, and a session that has simply gone quiet, which is the failure shape this
  product can least afford. Two candidate answers, not exclusive: **remove the case** by
  setting `PAGER`/`GIT_PAGER` so the results buffer is the pager (roadmap 22.7, and much
  the better answer wherever it applies, since the buffer is browsable and `less` is not);
  and **detect the state some other way** for programs that page or prompt without alt
  screen at all — a command that has produced output and then gone silent without ending
  is the observable shape, and it is the same quiescence signal the pacing policy already
  computes.
- Interactive-mode screen reading strategy: how the buffer/grid is exposed to the
  screen reader while a full-screen app runs (review cursor? live row announcements?).
- Non-visual tab/session navigation UX (switch keys, announcing which session is
  active, activity in background tabs — e.g. a long build finishing in another tab).
- Announcement channel model (tied to the B1 pacing work, where real output streams
  exist): should status announcements (failure, too-big, patience, alt-screen) live in
  a live region separate from auto-read output, and what are the interrupt/order
  semantics — polite vs assertive, does a failure interrupt output reading or wait its
  turn, do two regions read in a guaranteed order across NVDA/JAWS? A3 keeps a single
  polite region that appends each announcement so back-to-back messages are all spoken
  in order (the immediate `fail` clobber is gone); the separate-region question is
  deferred here. **Live evidence, 2026-08-15 (A3.1's NVDA run, captured through the
  screen-reader bridge):** two announcements appended within one tick are spoken as a
  *single concatenated utterance*, not as two. Stopping two commands at once produced
  one utterance reading "command stopped command stopped". So "the region appends and
  the reader speaks additions in order" is true about order but not about separation —
  back-to-back messages merge, and a listener cannot tell one announcement from two.
  This sharpens the open question: the choice is not only polite-versus-assertive and
  cross-region ordering, but whether announcements need an enforced boundary
  (separate nodes are not enough) and whether coalescing should be prevented in the
  view or upstream by the pacing policy. That case was fake-only (concurrent commands
  do not occur with a real shell), but the merging mechanism is general and applies to
  any pair — an auto-read chunk followed immediately by a failure, or too-big followed
  by patience. Also bears on B1's babble guard, which is the layer that would throttle
  a genuine flood.
  **Resolved by A5.2 (2026-08-16):** the announcer now serializes announcements — a
  queue drained one per turn, so no two announcements share a live-region mutation
  batch; two stops are spoken as two `command stopped` utterances. Coalescing was
  rejected (no new strings). The separation mechanism was measured through the
  screen-reader bridge rather than assumed, and the measurement changed the answer: a
  separate macrotask is not enough, because WebView2 batches accessibility updates per
  rendering lifecycle and mutations inside one batch still reach NVDA as a single
  live-region change. On NVDA 2026.1.1, 1 ms, 50 ms, and 75 ms all merged; 100 ms and
  250 ms separated. The landed drain spacing is 250 ms — about 2.5x the measured
  threshold, for margin against machine load and version drift — and a temporal gap
  alone proved sufficient, so the structural `<br>` separator was not needed. The gap
  is applied *between* announcements, never before one: an announcement arriving into an
  idle region cannot merge with anything, so it is not delayed at all, and only the
  second and later items of a burst wait.
  Render-before-announce holds via the controller's render-then-announce
  order plus the deferred drain; a commit/acknowledge gate is deferred to whichever PR
  makes buffer rendering asynchronous (rAF batching). The separate-region, polite-vs-
  assertive, and cross-region-order questions remain open.
  Noted en route: the end-of-command failure status is naturally an
  after-the-output thing, but patience, too-big, and alt-screen are inherently
  mid-command and cannot wait for command end.
- Reviewing past announcements in-app: region-only messages (failure, too-big,
  patience) are emptied from the live region after a short idle, so they are
  unreviewable in-app afterward (the screen reader's own speech history still holds
  them). If in-app review is wanted, the answer is writing status lines into the buffer
  as part of the block, not leaving stale text in the live region.
- Retrying the injection when the first attempt produces no markers (raised 2026-08-19
  while specifying B6, and recorded there rather than decided). Reliability case 2 above
  is **Decided** and now implemented: a session whose markers never appear is flagged,
  announced, and degraded honestly. What is not decided is whether it should first *try
  again* — an alternative injection, a different snippet, a second prompt — which would
  make case 2 rarer rather than merely honest. It needs `ShellAdapter` to exist (B5), and
  it belongs to a larger conversation about what a session's startup handshake actually
  is: how many attempts, how long each one may take against the grace period the user is
  waiting through, and whether a session that recovered on the second try should say so.
  Decided no earlier than B5.
- SSH auth UX: password prompts, key files, agent support, host key verification —
  all must be fully screen-reader-accessible flows.
- Password / no-echo prompts in non-interactive mode: typing a password into the
  local edit field would display and speak it. Remote no-echo state is unreliable to
  detect through ConPTY; likely a "secure input" toggle masking the field, possibly
  with heuristic detection of password-prompt text as a hint.
