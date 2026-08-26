# A9 — the window says what it is connected to

Roadmap entry 13.1, lane 1. **Agreed in conversation 2026-08-25**, raised by the user
directly after B5.6: having made the *prompt* audible again, the window itself still says
nothing about what it is.

## What is missing

Acter's window has a title bar reading "Acter" and a document with no heading at all. So:

- **Alt+Tab and the task bar say nothing useful.** Somebody with two Acter windows open — one
  on PowerShell, one on a WSL distribution — cannot tell them apart without entering one and
  running a command. The window title is the only thing the *desktop* reads out, and it is
  the one thing Acter has not been using.
- **The document has no top.** A listener arriving in the buffer with `Ctrl+Home`, or asking
  their reader to read the window, meets a results region with no statement of what this
  window is.
- **Connection state is invisible.** `ConnectionState` has existed in the protocol since A2
  and has never had a producer or a place to be shown. A session that is starting, one that
  is connected, and one whose far end has gone away are indistinguishable — which is the
  gap roadmap 23.7 filed from the other end, where the user heard "panel" and silence while
  PowerShell started.

## Decisions

### 1. Two titles, one source

**The operating system's window title**, which is what the desktop reads, and **an `h1` at
the top of the document**, which is what a reader meets inside it. Both say the same thing
and are set from the same value, because two titles that can disagree eventually will.

- Not connected: **`Acter`**
- Connected: **`Acter - PowerShell`**, `Acter - WSL: Ubuntu`, `Acter - Command Prompt`

The name after the dash is the connection's own label — the same string B5.4's catalogue
uses in the connect list, so what a user chose and what the window then calls itself are the
same words rather than two spellings of the same idea.

**An `h1` rather than a landmark or a `<header>` with text**, because a heading is what
`Ctrl+Home` and heading navigation land on, and because it gives the document the top it
currently lacks. It is level 1 and the command blocks are level 2, which is the structure
that was already implied.

### 2. A status region that says the three states out loud

A status bar showing **not connected**, **connecting**, or **connected**, and — when there is
one — the far end it is connected to.

**`role="status"`**, so a change is announced without stealing focus. That is the whole point
for this audience: the state changes while the user is doing something else, and a status
that must be sought out is a status nobody has. It is polite rather than assertive, so it
never interrupts output being read.

**And it is a real element in the document**, so it can be reviewed at any time with ordinary
reading commands. A listener who missed the announcement can go and read it.

### 3. `Connecting` becomes a state the domain can report

`ConnectionState` today is `Connected`, `Reconnecting`, `Disconnected` — written for a
transport that is either up or down. There is a fourth situation and it is the one the user
actually meets first: **the session is starting and is not usable yet.**

So `ConnectionState::Connecting` is added, and the session reports it from the moment the
transport is built until the far end says something. **This is what closes roadmap 23.7**:
the window opens saying "connecting", and a listener knows that waiting is correct rather
than hearing a panel and silence.

**What counts as connected is the first thing the far end says**, not the moment a process
was spawned. A shell that was launched and has not drawn a prompt is not a shell anyone can
use, and reporting it as connected would be a lie a listener acts on.

### 4. Disconnected is a real state, not an error

When the far end goes away — the shell exited, the pipe closed — the window says so and keeps
saying so, rather than announcing an error once and looking identical to a working session
afterwards. The title drops back to `Acter`, because there is no longer a connection to name.

### 5. What this entry does not do

- **No reconnection.** `Reconnecting` stays in the enum with no producer; a transport that
  can reconnect is SSH's, and it arrives with SSH.
- **No connection switching.** Choosing a different far end is B7 and A8. This entry makes
  the window describe whatever session it has.
- **No status beyond connection.** The working directory, the exit code of the last command
  and the session's mode are `Ctrl+Shift+S`'s, which DESIGN reserves for a status
  *announcement* on demand. A status bar that accumulated everything would be a second
  buffer nobody reads.

## Files touched

- `crates/acter-core/src/entities/protocol_common.rs` — `ConnectionState::Connecting`.
- `crates/acter-core/src/services/session.rs` — reporting connecting and connected, and
  disconnected when the far end ends.
- `crates/acter-app/src/routers/` — the connection's display name, from the same place the
  catalogue takes it.
- `ui/src/views/main_window.html` — the `h1` and the status region.
- `ui/src/adapters/window_title.ts` — setting both titles (new; role: adapter).
- `ui/src/adapters/status_bar.ts` — the status region (new; role: adapter).
- `ui/src/controllers/app.ts` — routing `ConnectionChanged` to both.
- Tests in each, plus an E2E assertion on the document title and the status text.

## Definition of done

- [ ] The window title and the `h1` both read `Acter` with no session and `Acter - <name>`
      with one, and they are set from one value.
- [ ] The status region reports not connected, connecting and connected, and announces
      changes without stealing focus.
- [ ] A session that is starting says "connecting" from the moment the window opens, which
      is roadmap 23.7's finding.
- [ ] Connected is reported when the far end first speaks, not when the process was spawned.
- [ ] A far end that goes away leaves the window saying so, with the title back to `Acter`.
- [ ] Unit tests in the domain, vitest for both adapters, an E2E test over the title and the
      status text.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests, vitest and E2E
      all clean.

## Accessibility checklist for the PR body

- [ ] With the window open and a shell starting, the listener is told it is connecting
      rather than meeting silence.
- [ ] When the session becomes usable, that is announced without interrupting anything.
- [ ] Reading the window (the reader's read-window command) names what this window is
      connected to.
- [ ] `Ctrl+Home` in the buffer lands on the window's own heading.
- [ ] The status can be read on demand, after its announcement has passed.
- [ ] Two Acter windows on different shells are distinguishable in the task switcher.
