# B7 — sessions that start at runtime

Roadmap entry 25, lane 2. Agreed in conversation 2026-08-23. Depends on 23.1 for the port
and reads better after 23.2 and 23.3, which give it something to list.

**What Connect needs, and what does not exist.** The composition root builds exactly one
session at startup, from `ACTER_SHELL` or `ACTER_TRANSCRIPT`, and `AppState` holds it as a
fixed `Arc<dyn SessionApi>`. There is no way to reach a second far end without restarting
the application, and the only way to reach a different one is to relaunch with a different
environment.

This entry makes a session something the user starts. A8 is the dialog over it.

## Decisions

### 1. Two commands, and the frontend attaches exactly as it does at startup

```rust
fn connectable(&self) -> Vec<Connectable>;           // what this machine offers
fn connect(&self, to: ProfileId) -> Result<Connected, String>;
```

`Connected` carries the new `SessionId` and the label of what was connected to. The
frontend then calls `attach_session` with that id — the same call it already makes at
startup, on the same channel.

**Re-binding the sink behind the frontend's back was rejected.** It would make `connect` a
command with an invisible second effect, and it would leave the frontend guessing when the
old session's events stop and the new one's begin. Explicit attach means the frontend
clears its buffer between the two calls, at a moment it chooses, and nothing arrives in a
buffer that still shows another shell's output.

### 2. `ConnectApi` is a driving port; making a session is a driven one

```rust
pub trait ConnectApi: Send + Sync {          // ports/driving
    fn connectable(&self) -> Vec<Connectable>;
    fn connect(&self, to: ProfileId) -> Result<Connected, String>;
}

pub trait SessionFactory: Send + Sync {      // ports/driven
    /// Build a live session for this profile, or say — in a sentence a listener can
    /// hear — why it could not be built.
    fn open(&self, profile: &ProfileId) -> Result<Arc<dyn SessionApi>, String>;
}
```

`ConnectService` implements `ConnectApi` and depends on `SessionFactory` plus
`InstalledShells` (23.3). The composition root implements `SessionFactory`, because
constructing a `LocalPty`, an `AlacrittyEngine` and a `SessionService` is naming concrete
implementations, and ARCHITECTURE allows exactly one place to do that.

The reward is that the whole of connecting — what is offered, what replacing means, what
happens when it fails — is testable with a fake factory and no process, no runtime and no
Tauri.

### 3. Connecting replaces the running session, and the old one really ends

DESIGN, decided in the same conversation: one session at a time in phase 1, tabs later.

**"Really ends" is the part with teeth.** Dropping the outgoing `Arc` has to reach
`LocalPty::drop`, which is what kills the shell. If any task, controller or router clone
outlives it, the shell survives, invisibly, and a user who connects five times has five
shells. So this is a test rather than an intention: after a replace, the previous
transport's read channel is closed and its far end is gone.

Two things that legitimately survive, recorded so they are not filed as leaks: a far end the
user proxied into is not in Acter's process tree (22.6), and the *frontend's* sink outlives
the session by design, since it belongs to the window rather than to the shell.

### 4. A failed connect is a sentence, and the session it failed to replace is untouched

Startup may still panic on a shell that cannot start — a window opening onto silence is
worse than one that does not open, and that reasoning is unchanged.

A connect is different in every way that matters: there is a user standing in front of it,
there is a working session behind it, and there is somewhere to put the answer. So `connect`
returns a speakable sentence, the running session is left exactly as it was, and the
frontend says the sentence. **The order of operations follows from that**: the new session
is built *before* the old one is dropped, so a failure costs the user nothing.

### 5. What is connectable is answered fresh, every time

Distributions get installed, PowerShell 7 gets installed, and a list computed once at
startup would be wrong for the rest of the session with no way for the user to notice.
`connectable()` asks `InstalledShells` each time the dialog opens.

Its shape:

```rust
pub struct Connectable {
    pub id: ProfileId,
    /// What the user hears and reads: "cmd", "PowerShell 7", "WSL: Ubuntu".
    pub label: String,
}
```

**The label is the profile's, not the adapter's** (23.1, decision 3), which is why two
entries can share one adapter.

### 6. The scripted sessions are in the list, in debug builds only

DESIGN has said since A3 that the scripted fake is a permanent, selectable session kind
rather than a launch-time environment variable, and this is where it becomes selectable.
The four built-in names — `builtin`, `builtin-by-byte`, `unmarked`, `unmarked-by-byte` —
become four entries.

Gated with `#[cfg(debug_assertions)]` at the factory and at the list, exactly as T2 gated
the embedded WebDriver and A3.2 gated the frontend recorder: a release build does not hide
them, it never constructs them.

`ACTER_SHELL` and `ACTER_TRANSCRIPT` keep working and keep choosing the *startup* session.
Nothing about launching changes here; what changes is that launching is no longer the only
way.

### 7. Announcing the change is the frontend's words and the backend's fact

The backend reports the label; the frontend owns the sentence, the way every pinned string
in `controllers/app.ts` already is. What a listener needs to hear is which far end they are
now talking to, because a window that looks identical and answers differently is the worst
outcome available here.

## Files touched

- `crates/acter-core/src/ports/driving/connect_api.rs`,
  `crates/acter-core/src/ports/driven/session_factory.rs` — the two ports (new).
- `crates/acter-core/src/entities/protocol_commands.rs` — `Connectable`, `Connected`,
  `ProfileId`.
- `crates/acter-core/src/services/connect.rs` — `ConnectService` (new).
- `crates/acter-app/src/container.rs` — the factory, the debug-gated profiles, `AppState`
  holding a session that can be swapped.
- `crates/acter-app/src/routers/session.rs` — the two commands.
- `ui/src/protocol.ts`, `ui/src/ports/backend_api.ts`, `ui/src/routers/tauri.ts` — the
  frontend side of the two commands.

## Definition of done

- [ ] `connectable()` lists cmd, every installed PowerShell edition, one entry per installed
      WSL distribution, and — in debug builds only — the four scripted sessions.
- [ ] `connect` builds the new session before dropping the old one, and a failure leaves the
      running session working and returns a speakable sentence.
- [ ] A replaced session's shell is gone: its transport channel closes and its far end
      exits. Tested, not assumed.
- [ ] Connecting twice in a row leaves exactly one shell running.
- [ ] `ConnectService` is tested with a fake factory and fake `InstalledShells`: no process,
      no runtime, no Tauri.
- [ ] The router tests cover both commands through the real IPC pipeline, as T1 requires.
- [ ] A release build constructs no scripted profile — verified the way T2's gate is.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests, vitest clean.

No accessibility checklist of its own: nothing here is user-facing without A8, which carries
the checklist for both.
