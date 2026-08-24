# B7 — sessions that start at runtime

Roadmap entry 25, lane 2. Agreed in conversation 2026-08-23, and **revised in the same
conversation** after the user proposed actions rather than a user interface. Depends on 23.1
for the port; reads better after 23.2 and 23.3, which give it something to list.

**What Connect needs, and what does not exist.** The composition root builds exactly one
session at startup, from `ACTER_SHELL` or `ACTER_TRANSCRIPT`, and `AppState` holds it as a
fixed `Arc<dyn SessionApi>`. There is no way to reach a second far end without restarting
the application.

## The idea that shapes this entry: actions, not a user interface

The problem it answers is A7's: **no suite in this project can drive a native menu.**
`MockRuntime` does not run the native webview and WebDriver drives only the webview, so a
design where "connecting" lives inside a menu handler is a design where connecting is
untested.

So connecting is a pair of **named actions** — `connectable` and `use` — and the menu is the
thinnest possible caller of them. The same actions are called by the launch path when
`--profile` names one (B8), and directly by tests, with no window, no webview and no screen
reader in the way. What remains untestable is then only the menu *widget*: whether an item
exists and fires, which the NVDA pass observes and which A7's pure menu definition already
covers structurally.

This is why the entry is worth its size. Making the session replaceable is perhaps forty
lines; making it replaceable **through a seam that can be measured** is the part that keeps
the next three entries honest.

## Decisions

### 1. Two actions, and the frontend attaches exactly as it does at startup

```rust
pub trait ConnectApi: Send + Sync {          // ports/driving
    /// Everything this machine offers, freshly asked each time.
    fn connectable(&self) -> Vec<Connectable>;

    /// Start this one and replace whatever was running, or say — in a sentence a
    /// listener can hear — why it could not be started.
    fn use_profile(&self, id: &ProfileId) -> Result<Connected, String>;
}
```

`Connected` carries the new `SessionId` and the label of what was started. The frontend then
calls `attach_session` with that id — the same call it already makes at startup, on the same
channel.

**Re-binding the sink behind the frontend's back was rejected.** It would make `use` a
command with an invisible second effect and leave the frontend guessing where one session's
events stop and the next one's begin. An explicit attach lets the frontend clear its buffer
between the two calls, at a moment it chooses, so nothing lands in a buffer still showing
another shell's output.

### 2. Making a session is a driven port, so the domain never names a shell

```rust
pub trait SessionFactory: Send + Sync {      // ports/driven
    fn open(&self, profile: &ProfileId) -> Result<Arc<dyn SessionApi>, String>;
}
```

`ConnectService` implements `ConnectApi` and depends on `SessionFactory` plus
`InstalledShells` (23.3). The composition root implements `SessionFactory`, because
constructing a `LocalPty`, an `AlacrittyEngine` and a `SessionService` means naming concrete
implementations and ARCHITECTURE allows exactly one place to do that.

The reward is that the whole of connecting — what is offered, what replacing means, what
happens when it fails — is tested with a fake factory: no process, no runtime, no Tauri.

### 3. The window starts unconnected, and says so

DESIGN, decided in the same conversation. Nothing is spawned until a profile is used.

**An empty window is a state with obligations**, and they are the substance of this entry's
accessibility work:

- On open with no profile, the session state is announced — not connected, and that the
  Acter menu is where to go.
- **A line submitted while unconnected is answered, never swallowed.** The submission is
  refused with the same sentence, and the edit field keeps the text so nothing the user
  typed is lost.
- The results buffer is empty and says nothing else. There is no fabricated prompt and no
  placeholder block: a heading a listener can navigate to must correspond to something that
  ran.

This is a new `SessionApi` state rather than a session that exists and does nothing.
Modelling "unconnected" as a live session with a dead transport would make every other part
of the system reason about a far end that was never there.

### 4. Using a profile replaces the running session, and the old one really ends

One session at a time in phase 1; tabs later, as DESIGN has it.

**"Really ends" is the part with teeth.** Dropping the outgoing `Arc` has to reach
`LocalPty::drop`, which is what kills the shell. If any task, controller or router clone
outlives it, the shell survives invisibly, and a user who connects five times has five
shells. So it is a test rather than an intention: after a replace, the previous transport's
read channel is closed and its far end is gone.

Two things legitimately survive, recorded so they are not filed later as leaks: a far end
the user proxied into is not in Acter's process tree (22.6), and the *frontend's* sink
outlives the session by design, since it belongs to the window.

### 5. A failure leaves the running session untouched

`use_profile` returns a speakable sentence; the frontend says it; the session that was
running is still running and still attached. **The order of operations follows from that**:
the new session is built *before* the old one is dropped, so a failure costs the user
nothing — not even the shell they were in.

### 6. What is connectable is answered fresh, every time

Distributions and PowerShell editions get installed while an application is running, and a
list computed once at startup would be quietly wrong with no way for the user to notice.

```rust
pub struct Connectable {
    pub id: ProfileId,
    /// What the user hears: "cmd", "PowerShell 7", "WSL: Ubuntu".
    pub label: String,
}
```

**The label belongs to the profile, not the adapter** (23.1, decision 3), which is how two
entries can share one adapter.

### 7. The scripted sessions are in the list, in debug builds only

DESIGN has said since A3 that the scripted fake is a permanent, selectable session kind
rather than a launch-time environment variable, and this is where it becomes selectable.
The four built-in names become four entries, gated with `#[cfg(debug_assertions)]` at the
factory and at the list — exactly as T2 gated the embedded WebDriver: a release build does
not hide them, it never constructs them.

`ACTER_SHELL` and `ACTER_TRANSCRIPT` keep working in debug builds as they do today, because
the suites and the manual passes use them; B8's `--profile` is what replaces them, and they
are retired when it has.

## Files touched

- `crates/acter-core/src/ports/driving/connect_api.rs`,
  `crates/acter-core/src/ports/driven/session_factory.rs` — the two ports (new).
- `crates/acter-core/src/entities/protocol_commands.rs` — `Connectable`, `Connected`,
  `ProfileId`, and the unconnected state.
- `crates/acter-core/src/services/connect.rs` — `ConnectService` (new).
- `crates/acter-app/src/container.rs` — the factory, the debug-gated scripted profiles, and
  `AppState` holding a session that can be swapped or absent.
- `crates/acter-app/src/routers/session.rs` — the two commands, and submission while
  unconnected.
- `ui/src/protocol.ts`, `ui/src/ports/backend_api.ts`, `ui/src/routers/tauri.ts`,
  `ui/src/controllers/app.ts` — the frontend side, including the two pinned strings the
  unconnected state needs.

## Definition of done

- [ ] `connectable()` lists cmd, every installed PowerShell edition, one entry per installed
      WSL distribution, and — in debug builds only — the four scripted sessions.
- [ ] `use_profile` builds the new session before dropping the old one; a failure leaves the
      running session working and returns a speakable sentence.
- [ ] A replaced session's shell is gone: its transport channel closes and its far end
      exits. Tested, not assumed. Connecting twice leaves exactly one shell running.
- [ ] An unconnected window announces that it is unconnected, and a line submitted into it
      is refused with a sentence and left in the edit field.
- [ ] `ConnectService` is tested with a fake factory and fake `InstalledShells` — no
      process, no runtime, no Tauri — including the failure path and the replace path.
- [ ] Router tests cover both commands through the real IPC pipeline, as T1 requires.
- [ ] A release build constructs no scripted profile, verified the way T2's gate is.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests, vitest clean.

## Accessibility checklist for the PR body

This entry has user-facing behaviour of its own even before A8 gives it a menu, because the
unconnected window is what every launch now opens with. Agent-observable through the
screen-readers bridge, `user` persona:

- [ ] Launching with no profile announces that there is no session and where to connect.
- [ ] Submitting a line while unconnected is answered aloud, and the typed text is still
      there afterwards.
- [ ] The results buffer contains nothing navigable — no heading, no empty block.
