# Acter — Design Document

Acter is an accessibility-first terminal for screen reader users. Rust, self-contained,
Windows first, macOS second (see the macOS section below), GUI via an HTML frontend.

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

### Far-end-line mode (added 2026-08-31, and it sits between the two)

- The edit field holds no text, and every keystroke goes to the far end as it is typed:
  arrows, Tab, Enter, and Ctrl plus any key. The results buffer is unchanged, so the
  session is still conversational and still browsable with ordinary reading commands.
- It exists because the far end past the shell Acter spawned — an `ssh`, a `wsl`, a
  container, a REPL — has its own line editor, its own history and its own completion,
  and those are the ones the user wants while they are there.
- It is not interactive mode by another name. Interactive mode is about how the *screen*
  is presented and needs a renderer; this is about who owns the *line* and needs none.
  Detail under "Edit field ownership" below.

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

### The menu bar is in the document — **Decided, revised 2026-08-24**

**This reverses the decision recorded here on 2026-08-23, on measurement rather than on
preference.** That decision said the menu bar was native — a Win32 `HMENU` under Tauri's
`MenuBuilder` — and rejected an in-page ARIA `menubar` "not narrowly", on the grounds that
it would put a browse-mode/focus-mode switch in front of the one control every user of this
product must reach. A7 opened with the measurement that decision assumed the answer to, and
the answer came back the other way.

**What was measured** (spec A7 carries the numbers and NVDA's own stacks): a native menu
bar attached to Acter's window does everything the old decision expected — Alt reaches it
from inside the webview, NVDA announces the bar, arrows and Enter behave — and **opening it
freezes NVDA for twenty to sixty-eight seconds**, every time. The reader's main thread
stops, not merely its announcement. It reproduces in a release build, with the keystroke
injected from outside NVDA, with the tester's add-ons disabled, and finally in a **vanilla
Tauri application built from Tauri's own window-menu tutorial**, which has no Acter code in
it at all. Alt+Tab out of the same window is clean, so it is the native menu *taking* focus
that does it. NVDA's log names the mechanism: synchronous accessibility calls into the
WebView2 renderer, each cancelled by COM's message filter after ten seconds.

So the trade the old decision weighed was not the real one. It compared a mode switch
against E2E testability; the actual comparison is a mode switch against a screen reader
that dies for half a minute whenever the user opens a menu.

**The menu bar is therefore a WAI-ARIA `menubar` in the page**, and the mode problem the
old decision was right to fear is answered rather than accepted: the bar is wrapped in
`role="application"`, so the arrows belong to the widget whatever the reader's automatic
focus mode setting happens to be. Measured immediate at every step, with no freeze.

**Two ways in, both measured working**: **F10**, the platform's own "give me the menu bar",
and **Alt on its own** — answered on keyup and disarmed by any other key, a click, or the
window losing focus, so Alt+Tab and Alt+F4 pass through untouched. The key that made a
native menu worth wanting now works without one.

**Nothing else in the window is an application.** The command line was tried that way and
reverted the same day: wrapping it does force focus mode, but it removes browse mode from
the one place a listener uses it deliberately — turning focus mode off at the edit field
and arrowing up to hear the tail of the last command. The results buffer is a document for
the same reason, and more obviously. The mode at the command line is the reader's own
business; only the menu bar, where there is nothing to read and the arrows must reach the
widget, is stated as an application.

**Windows only.** This exists because Windows is where a native menu freezes the reader. On
macOS a menu belongs in the system bar and not in the window at all, and Linux is likely to
want its own answer, so the backend answers which platform this is and the frontend removes
the region entirely off Windows.

**What this buys back, unexpectedly**: the menu is now inside the webview, so WebDriver can
drive it end to end. "Nothing in this project can test the menu" was written about the
native design and stops being true.

Two menus, and no more until something earns one:

- **File** — Connect (see below), Exit.
- **Help** — About Acter. A menu holding one item rather than a top-level item that acts,
  because a menu bar entry that fires instead of opening is a surprise to anyone navigating
  by arrow keys.

### Dialogs are HTML modals in the window — **Decided**

A `<dialog>` in the main window reuses the focus discipline, the announcer and the test
suites the product already has, its text is browse-mode readable and copyable, and both
vitest and WebDriver can drive it. Native message boxes would add a dependency and a
surface neither suite can reach. Since the menu bar itself became a document (above), this
is no longer a contrast — it is the same reasoning applied twice.

**One thing the platform does not do for us**, measured through NVDA on 2026-08-24: in a
modal dialog with a single focusable control, Tab leaves that control for the dialog's own
document, which drops the reader back into browse mode. A dialog traps Tab explicitly
rather than trusting the element to.

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

### Connect is a dialog: a kind, then what that kind needs — **Decided, revised 2026-08-24**

**This reverses the submenu decision recorded here on 2026-08-23**, which said "no dialog:
connecting needs no more than a choice". It does. SSH needs a host, a port and a user, and
those are a form rather than a choice — a submenu cannot hold one, and shipping a submenu
for the easy kinds beside a dialog for SSH would leave the product with two ways to do one
thing. The old decision's argument was navigational and remains true as far as it goes: a
conditional second control is harder to navigate non-visually than a list of equals. It is
answered below rather than ignored.

**File → Connect opens one dialog**, and it has two parts:

- **A list of connection kinds.** Today: cmd and PowerShell. Later: WSL, SSH, and saved
  connections.
- **A panel below it holding whatever that kind needs.** For cmd and PowerShell that is
  nothing at all, and the dialog is a list and a Connect button. For WSL it is the list of
  distributions installed on this machine. For SSH it is host, port, user and the rest.

**The panel changes under you, so it announces itself.** That is the whole of the old
decision's worry, and it is a solved problem rather than an accepted cost: changing the
kind announces what the panel now holds, and Tab from the kind list lands in it. A silent
swap of controls is the trap; a spoken one is a form.

**Saving comes after it works, not before.** A connection is *probed* — attempted for real —
and only a connection that came up is offered for saving. This is what turns profiles from
files a user hand-edits into something the application creates: the saved connections
appear as their own kind in the list, and the profile section above stops describing a
thing only an editor can make. It also settles the ordering: **the JSON profile store is
built after WSL lands**, because a save flow with only cmd and PowerShell behind it saves
nothing a user could not retype in a second.

**A failed connection is spoken and leaves the running session alone.** Connecting at
runtime makes a shell that will not start an ordinary event rather than a startup panic,
and that is the requirement this dialog puts on the action behind it.

The frontend hardcodes no list. The backend answers what can be connected to: shells
discovered on this machine — cmd, each installed PowerShell edition, one entry per
installed WSL distribution — joined with the connections the user has saved. The scripted
fake sessions join the list in debug builds, which is what the profiles section promised:
the fake is a permanent, selectable session kind rather than a launch-time environment
variable.

### What macOS offers is a Terminal and SSH — **Decided 2026-08-31**

**Two kinds and no more.** A shell on this Mac, called Terminal, and SSH. cmd, PowerShell
and WSL are Windows things: they are absent from a macOS catalogue entirely rather than
listed as unavailable, because `WSL (not available)` on a Mac — with instructions to install
Windows — is precisely the absurdity the not-available label exists to avoid on the platform
where it means something.

**Terminal is one row with the shells as its variants**, the shape PowerShell's editions and
WSL's distributions already have. The panel lists what `/etc/shells` names, with the
account's own login shell first and marked as the default, so Enter on the row with nothing
picked starts the shell a Terminal.app window would have started. One row, however many
shells a Mac happens to have — a listener arrowing the kinds meets "Terminal" once.

**SSH is the same SSH.** Acter speaks the protocol itself rather than running `ssh`, which
was decided for accessibility reasons in B9 and pays a second time here: the kind that is
hardest to port is the one that needed no porting at all.

**The signature check comes with the Terminal row rather than after it.** Off Windows the
`Signatures` port falls back to an implementation that vouches for nothing, which is the
right refusal and the wrong experience if it ships alone: every local connection would raise
a security dialog, including for a `/bin/zsh` that Apple signed, and a dialog that always
fires is a dialog a listener learns to dismiss. So macOS gets its own answer to "who signed
this file" in the same entry that gives it a file to start.

### macOS has no browse mode — it has Quick Nav, and Quick Nav is two toggles

**Measured 2026-09-01, and recorded here because the product has no answer to it yet.**
Every rule in this document about browse and focus mode is NVDA's, and VoiceOver's
equivalent is not the same mechanism wearing a different name.

**Quick Nav is two independent toggles.** Arrow-key Quick Nav makes the plain arrows move
the VoiceOver cursor instead of reaching the application. Single-key Quick Nav makes `h`,
`b` and `l` jump by element type. They normally travel together and are toggled by pressing
Left Arrow and Right Arrow at the same time. A preference splits them —
`SCRCUserDefaultsIndependentSingleLetterQuickNavEnabled`, in the VoiceOver group container's
`com.apple.VoiceOver4/default.plist` — and with the split in force the letters navigate
while the arrows still reach the application. Even with Quick Nav on, Up and Down inside a
text field belong to that field.

**Why this is Acter's problem and not trivia.** Quick Nav on is what lets a listener read
the results buffer, and it is exactly what stops them typing a command. That is the same
tension browse versus focus mode carries on Windows, where this document's answer is that
the mode is the reader's own state, invisible to a web frontend, and must not be detected.
Whether that answer survives on a reader whose two halves can be toggled apart is unknown:
nobody has driven the real window under either combination yet.

**So it is not decided, and it is not this entry's to decide.** It has a roadmap entry of
its own after M3, where the measurements can be taken against a macOS build that has a
buffer and an edit field to be in the wrong mode for.

### Acter starts unconnected, and `--profile` is the only switch — **Decided**

Launched with no arguments, Acter opens a window with **no session**: nothing is spawned
until the user connects. Launched as `acter --profile <name>`, it starts that profile's
session immediately.

**An unconnected window must say so.** It announces that it is not connected and what to do
about it. A window that opens onto silence is the failure shape this product can least
afford.

### The window has two windows, and the connected one is the terminal window — **Decided 2026-08-26**

**A window with no session shows no terminal window**: no results buffer, and no edit field.
It holds a line saying it is not connected and a **Connect button**, which is where focus
lands when the window opens. With a session it holds the terminal window — the results buffer
and the edit field.

**They are two windows swapped as units, not one window whose controls appear and vanish.**
Exactly one is in the document at any moment, and once a session has run the empty one never
comes back: what answers a session ending is the terminal window's own ended state, which
keeps the buffer and replaces the edit field with a Connect button of its own. To a screen
reader `hidden` and "absent" are the same thing, so what matters here is the model rather
than the mechanism.

This supersedes the rule above about a line submitted before connecting being answered: with
no edit field there is nowhere to type one. The answer survives in the protocol as the
refusal a submission gets when it names a session that has since ended, which is a race
rather than a thing a user can do on purpose.

**Why a button rather than the menu.** The earlier wording sent the listener to the Acter
menu, which is a route to describe rather than a thing to do — and describing a route is what
this product exists to stop doing. One control, under focus, and Enter connects. The menu
item stays, because a menu is where a user looks for a command they already know.

**Why the empty buffer and field had to go.** A region holding nothing is something a
listener arrows onto and hears nothing useful from, and a field that can submit nothing is a
control they have to pass to reach the only thing that would help them. Two obstacles in
front of the one action available is the shape of an interface designed by looking at it.

**A new connection clears the buffer; a disconnection does not.** History is kept across the
end of a session, because it is the record of what happened — and never joined across two,
because a transcript stitched from two shells is a transcript of something that never
happened.

**When the far end goes away, the buffer stays and the edit field goes.** The buffer is by
then the record of what happened, and a user who typed `exit` by accident must not lose it;
the field has nothing left to submit to. Focus is rescued rather than stolen: it moves into
whatever is now showing only if it was in what just went away, so somebody reading the buffer
when their shell exits keeps their place.

**A dialog's contents sit in an application region.** NVDA reads a document in browse mode,
where the arrows move its own cursor and Enter acts on that cursor rather than on the focus,
and it switches to focus mode by itself only if the user left that setting on — a setting
rather than a guarantee. A dialog whose arrows depend on somebody's configuration is not a
dialog. The role goes on a container inside the `<dialog>`, which must keep announcing as one.
The cost is that prose inside cannot be arrowed, so read-only text in a dialog is focusable;
About keeps no such region, being four paragraphs and a button with no widget keys to protect.

**The terminal window is a unit, because tabs will make it many.** Phase 1 has one session
and therefore one terminal window; the buffer and the edit field are grouped as one thing in
the document rather than left as siblings, so the day a tab holds each of them is a change in
one place. The rules above are then per-tab: a tab whose session ended keeps its buffer and
loses its edit field, and a window with no tabs at all is the Connect button.

**Command-line arguments are parsed, never printed.** Acter is a windowed binary with no
console attached, so `--profile something-that-does-not-exist` cannot report itself on
stdout — and should not want to. The window opens, unconnected, and *says* what was wrong
with the name. Failures belong where the user is, spoken, which is the same rule the rest
of this document applies to shells that will not start.

There is deliberately **no `create profile` on the command line**: profiles are files, and
creating one from a shell nobody can see the output of is not a workflow this audience
needs. Editing them is hand-editing today and an in-app flow when one earns its place.

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

   **Amended 2026-08-31.** All of layer 2 describes the edit field while Acter owns the
   line. In far-end-line mode the field's native meaning *is* "send it": every key that
   is not layer 1 goes to the far end, plain Ctrl+C included — which is the interrupt by
   another road, the same `0x03` on the same wire, so the Decided rule above is honoured
   rather than displaced. The selection half of the test is not weakened either, because
   a field holding no text can hold no selection. Layer 3 is then what far-end-line mode
   already does, and what interactive mode adds on top of it is the screen, not the keys.
3. **Interactive mode passes everything that isn't layer 1 to the app**, including
   plain Ctrl+C (SIGINT via PTY), Alt combos (Meta keys), and Escape.

### Default bindings

- **Ctrl+Shift+E** — toggle interactive / non-interactive mode. **Decided.** (Moved
  from plain Ctrl+E, which collided with readline/nano/emacs end-of-line; terminal
  apps cannot receive Ctrl+Shift combos, so the collision vanishes.)
- **Ctrl+Shift+K** — hand the keyboard to the far end, and take it back: toggles
  far-end-line mode. **Decided 2026-08-31**; the letter is the one arbitrary part of it.
  It is a second binding rather than a reuse of Ctrl+Shift+E because the amendment under
  "Edit field ownership" split what used to be one switch into two independent ones, and
  this is the half that needs no renderer. The toggle is announced, and the announcement
  says what the user gains and loses rather than naming a mode: keys now go to the
  program, and history and completion are the program's; or keys now go to Acter, and
  history and completion are back. Ctrl+Shift+S reports which one is on.
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

**Amended 2026-08-31: that list is two independent switches, not one bundle**, and only
one of them needs phase 2. *Who owns the line* is keyboard routing, and it ships as
far-end-line mode with no renderer at all. *How the screen is presented* is the grid
renderer and the screen-reading strategy, and stays behind the phase 2 gate. Separating
them makes `ssh`, `wsl`, containers and REPLs usable — with the far end's own history and
completion — long before anything full-screen exists, and it is why the roadmap now
carries those entries outside that gate.

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

**Amended 2026-08-31: the line has two possible owners, and the user chooses — Decided.**
The section above is right about what may never happen and wrong about how many states
there are. Everything it claims is true of the shell Acter spawned and false of everything
past it. Inside an `ssh`, a `wsl`, a container or a REPL the far end has its own line
editor, its own history and its own completion, and those are the ones the user actually
wants: Acter's history holds lines typed at every far end this profile ever reached, and
Acter's completion completes this machine's paths. Substituting locally is right where
Acter is the shell's client and wrong the moment something else is. It is the same fault
line as marker injection — past the first far end, Acter is blind — reached from the
keyboard instead of from the output.

So there are two states, named for who owns the line being edited:

- **Local line** — everything described above, and the default in every session. The field
  owns the line, up and down are Acter's history, Tab is Acter's completion, and nothing
  crosses until Enter.
- **Far-end line** — the field owns nothing and holds no text. Every keystroke that is not
  layer 1 goes to the far end as it is typed: arrows, Tab, Enter, and Ctrl plus any key.
  What the user hears in reply comes from the row the far end redrew, under "A row that
  changed is an answer" below.

**Why the choice cannot be made key by key, which is what makes this a state rather than a
setting.** The obvious shape — pass up, down and Tab, keep the rest local — does not
survive contact. Those keys act on *a* line buffer, and in that shape there are two. A user
who types `car` into the field and presses Tab asks the far end to complete a line the far
end has never seen. A user who presses up recalls a line into the far end's buffer while
the field still holds its own, and the next Enter sends ours on top of theirs. That is a
corrupted command line, not a rough edge. Any key that edits a line must reach whichever
side owns the line, so ownership moves whole or not at all.

**The invariant survives, for the reason it was written.** What it forbids is a field that
renders remote state — asynchronous echo and a remotely-owned caret that no screen reader
can track. A far-end-line field renders nothing whatsoever: it holds no text, and it is a
place to aim keys rather than a place text lives. The mirroring rejected wholesale above
stays rejected, because this is not that.

**Never inferred — Decided.** The state is toggled by the user and by nothing else. There
is no fact to infer it from: the far ends that need it announce nothing, which is the
substance of the open question about programs that need keystrokes without ever entering
the alternate screen. And a state that changed itself would change what a key does between
one session and the next, which is the reason keybindings are global rather than
per-profile.

**What it costs, told to the user once rather than left to be discovered.** No local
history and no local completion while it is on; the far end's answer instead. Nothing is
submitted, so nothing opens a command block, nothing earns a heading, and nothing enters
Acter's history — that falls out of the existing exclusion rule rather than needing a new
one, but a user who runs a whole `ssh` session this way ends with an Acter history holding
none of it. And a slow far end is felt on every character, which is what the far end owning
the line means, and why the toggle has to be cheap to reach in both directions.

**Layer 1 is untouched in either state.** Ctrl+Shift is always Acter's, so the way back is
always pressable. Ctrl+Shift+Space still sends one literal keystroke without changing
state, which is the right tool for a pager or a bare `[y/N]` — neither of those edits a
line, so neither needs ownership to move.

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

## A row that changed is an answer — **Decided**

(Agreed in conversation 2026-08-31, with far-end-line mode above.) Once keystrokes reach
the far end, the far end's reply is not text it appends but a row it redraws. Up does not
emit a line; it repaints the row the cursor is on with a different one. Tab does not emit
a line; it extends it. So everything Acter can say about such a key comes from comparing
that row before and after — the differ this project has so far chosen not to build. It is
not avoidable once keys pass, and it is far smaller than the thing that was avoided.

**The same rewrite is churn or an answer depending on whether the user caused it**, and
Acter knows which without a heuristic: a keystroke arrived either as an Enter-submission
or as a pass-through, and that is a fact rather than an inference. A spinner repainting on
its own stays churn and stays unspoken, exactly as the identified-lines decision says. A
row that changes within the settling window after a key Acter sent is the answer to that
key, and is spoken.

Three bounds keep it small, and all three already exist:

- **Only after a key Acter sent.** Nothing is diffed continuously, and no timer runs while
  the user is not pressing anything.
- **Only when it settles**, on the quiescence clock the pacing policy already computes.
- **Only the cursor row**, which is where the whole of line editing happens.

The rule is then: *after a passed key that is not a printable character, if the cursor
row's content changed once it settles, speak the row.* Up gives `cargo test --all`. `car`
then Tab gives `cargo`. Ctrl+U gives an empty row, and what to say about that is a string
question rather than a mechanism one.

**Measured 2026-08-31, against `bash` inside WSL through a real pseudoconsole, and the
measurement corrects the rule in two places.**

*First, "speak the row" is wrong and would read the prompt aloud on every press.* What
comes back on the wire when the recalled line changes is `ESC [ 4 ; 35 H` followed by the
few characters that differ — readline repaints from the column the line starts at, and the
row the engine then reports is `"marlon@splyt:/mnt/c/Users/marlo$ exit"`, prompt included.
What a listener wants is `exit`. The rule is therefore **the row from the anchor column
onward**, where the anchor is the column the cursor sat at when the far end finished
drawing its prompt — which is not a new concept but the same anchor B4.9 already takes at
the instant of submission, used for a second purpose.

*Second, and this one would have shipped broken:* the first up arrow at a fresh prompt
produces **no rewrite at all**. The row is `…$ ` and readline simply writes the recalled
line after it, so the engine reports an *append*, not a revision. Only the second and
later presses rewrite, because only then is there something to overwrite. A rule keyed on
"a line was rewritten" would therefore be silent on the commonest press of the commonest
key. The comparison is against the row's content, and both kinds of change count.

With those two corrections the measured behaviour is exactly what is wanted: up gives
`echo acter-history-one`, up again gives `exit`, Ctrl+U gives an empty row, and typing
`ech` then Tab gives `echo` — where Tab's own contribution on the wire was the two bytes
`o ` and would have been meaningless spoken on its own.

**Printable characters need no diff at all.** The reader speaks them as they are typed, and
the far end's echo of them is suppressed by the positional rule already built for submitted
lines: text appended to the cursor row after Acter wrote to it is the echo. That rule
generalises from a line to a character unchanged, which is why this costs one comparison
rather than one subsystem.

**And there is no differ, which the measurement settled rather than argued.** The engine
has emitted identified lines with `Appended` and `Rewritten` revisions since B3, and it
already suppresses a repaint that changed nothing: a `gh` prompt redrawing four rows after
an arrow produced exactly two items, for the two rows whose *text* differed. So what this
section calls a diff is a policy over events that already exist, and the machinery whose
absence made "build a differ" sound expensive turns out to have been built for another
reason three entries ago. What was missing was never the comparison. It was permission to
speak the result.

**Structure is untouched by any of it.** A keystroke driving a running program earns
neither a heading nor an echo line, and whatever it produces appends to the block already
open. Twenty presses of `space` through a diff must not produce twenty headings called
"space". That was already this document's answer to the open question below; it is now
Decided, and it is a decision Acter makes without a heuristic because the category of a
keystroke is known rather than guessed.

**What the rule does not cover is a repaint of several rows** — the shape of a `gh`
selection prompt, and of every widget that draws a list and moves a highlight through it.
The axis that matters turns out not to be "the cursor row versus the whole grid" but **how
many rows changed** after a key Acter sent:

- none changed: say nothing;
- rows appended below: ordinary output, which autoread already handles;
- the cursor row alone: line editing, as above;
- a few rows in place: a widget whose selection moved, and the row that gained the
  selection is what a listener needs;
- most of the screen: a full repaint, which belongs to interactive mode and its renderer.

One mechanism with a threshold, not five mechanisms. Only the first three are specified.
The fourth is an experiment before it is a rule, because whether a text diff can see a
selection move at all depends on whether the highlight is drawn with a marker character or
only with colour — the grid carries attributes and the buffer does not — and that is
measured rather than assumed (roadmap entry 30).

**Measured 2026-08-31, and the fourth bucket survives.** `gh repo create`'s selection
prompt draws its highlight as a `>` in the text of the row, with colour *as well as* rather
than *instead of*, so a text comparison sees the selection move. It takes no alternate
screen, as expected. Each arrow changes exactly two rows — the one that lost the marker and
the one that gained it — and the engine reports both as revisions of stable line ids, while
the header row it repaints identically each time produces nothing. So the rule to write is
"speak the row that gained the marker", and the attribute-aware path the colour case would
have forced is not needed for this program. One program is not a population, and entry 30
keeps that caveat.

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

  **Answered 2026-08-31, in two halves, and one of them was already written here.** A
  keystroke can be sent without leaving the conversational view in two ways: one at a time
  with Ctrl+Shift+Space, which is what a pager's `space` and a bare `[y/N]` want since
  neither edits a line; and for as long as the user wants with far-end-line mode
  (Ctrl+Shift+K), which is what a far end's own line editor wants. The paragraph above
  about structure was right and is now Decided under "A row that changed is an answer".
  What was missing, and is the substance of the amendment, is that the sticky form cannot
  be built key by key — see "Edit field ownership".

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

  **Sharpened 2026-08-31 by the `gh` case, and the population is larger than pagers.** An
  inline selection prompt — `gh pr create` asking a question, arrows moving a highlight — is
  the same shape as `less -X`: it wants raw keys, it never takes the alternate screen, and
  it announces nothing. Setting `PAGER` removes the pagers and does nothing for this, so the
  case is permanent rather than transitional. The consequence for the design is that "this
  is an interactive program" is not a category Acter can compute, which is exactly why
  far-end-line mode is toggled by the user and never inferred. What is left to build is not
  detection but a *hint*: the quiescence signal named above, said once, so a user whose
  session has gone quiet is told the program appears to be waiting and how to hand it the
  keyboard. Roadmap entry 29 — and its difficulty is false positives, since a command that
  finished also goes quiet, not the signal itself.
- **Does a field holding no text still have to be an application?** (Raised 2026-08-31
  with far-end-line mode.) Arrows reach a web frontend only in focus mode, and Acter does
  not detect the reader's mode, by decision. `role="application"` was tried at the command
  line and reverted the same day, because it removed browse mode from the one place a
  listener uses it deliberately — turning focus mode off at the edit field and arrowing up
  to hear the tail of the last command. That reason is about a field with text in it. A
  far-end-line field has none, so there is nothing there to browse and the reversal's ground
  does not obviously hold. Whether the field becomes an application while the mode is on,
  and what a reader says when it lands on an edit field that is permanently empty, are both
  measurable on the bridge before any frontend work exists.

  **Measured 2026-08-31 on NVDA 2026.1.1 through the bridge, standing in as an ordinary
  user, and it answers the first half and complicates the second.** On a plain empty edit
  field NVDA announced it cleanly as name and role — "Plain command line, edit", with no
  "blank" — and then **did not switch to focus mode**, so the up arrow moved the browse
  cursor to the previous heading and the page never saw the key at all. The same field
  inside a `role="application"` region received up, down, Tab and Ctrl combinations, every
  one of them. So the mode does need the region, and the earlier reversal's ground does not
  hold where there is no text to browse.

  What it complicates is the element. An `<input>` inside that region makes NVDA say
  "blank" *before every arrow press*, because the caret moved in an empty text field — so
  the listener would hear "blank" ahead of whatever Acter says, on every keystroke, forever.
  The answer is likely that the key sink should not be a text input at all but a focusable
  element with no text in it, inside the application region. That is entry 28's to settle,
  and it is now a choice between two known behaviours rather than an open question.
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
