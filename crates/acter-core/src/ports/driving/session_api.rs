//! Port (driving): what the frontend may ask of a session. Services implement it,
//! routers depend on it through `Arc<dyn SessionApi>` — which concrete backend, and
//! which transport under it, is chosen only in the composition root.

use std::sync::Arc;

use crate::{EventSink, KeyAck, KeyPress, LineOwner, SessionId, SubmitAck};

/// The session domain's actionable surface. Methods are synchronous
/// (sync-core/async-edges; also keeps the trait dyn-compatible without async_trait).
/// The `SessionId` is carried on every call even though Phase 1 has one session
/// (Decided: commands carry `session_id` as an argument).
pub trait SessionApi: Send + Sync {
    /// Bind the event sink the session emits through. Called once at startup when the
    /// frontend establishes its Channel; later events for this session flow to `sink`.
    fn attach_session(&self, session: SessionId, sink: Arc<dyn EventSink>);

    /// Accept a submitted line. Returns immediately with the correlation id every
    /// later event about this command carries — an invoke never waits on the shell.
    fn submit_command(&self, session: SessionId, line: &str) -> SubmitAck;

    /// Report a keystroke the frontend did not consume, and answer what became of it.
    ///
    /// The key, not the meaning (spec B6, decision 4): the domain owns the binding
    /// table, so adding an intent later is a backend change with no frontend release,
    /// and `Ctrl+Shift+Space` — a Decided binding meaning "send the next keystroke
    /// literally" — stays expressible, which an intent-shaped port would not have made
    /// it. There is deliberately no `interrupt()` beside this: a second, meaning-shaped
    /// method would put the binding table back in the frontend.
    ///
    /// No `command_id`: the service targets whatever is running, because a
    /// frontend-supplied id can only be stale — the command may have ended between the
    /// keypress and the invoke. Returns immediately, like every other invoke.
    fn send_key(&self, session: SessionId, key: KeyPress) -> KeyAck;

    /// Hand the line to the far end, or take it back (spec 28, decision 1).
    ///
    /// **The state lives here rather than in the frontend because the domain is what needs
    /// it**: which bytes a key becomes, whether Enter opens a block, and which row goes in
    /// front of the listener all depend on it. A frontend holding it would need a second
    /// binding table to act on it, which is the seam B6 decision 4 exists to prevent.
    ///
    /// Toggled by the user and by nothing else — never inferred. The far ends that need it
    /// announce nothing, and a state that changed itself would change what a key does
    /// between one session and the next (DESIGN, "Edit field ownership").
    fn set_line_owner(&self, session: SessionId, owner: LineOwner);

    /// Paste text into the far end's own line editor.
    ///
    /// **Wrapped in `ESC[200~` and `ESC[201~` when the far end asked for that and sent bare
    /// when it did not** (spec 28, decision 10), which is why this is an invoke of its own
    /// rather than a run of [`Self::send_key`] calls: only the emulator knows whether
    /// bracketed paste is on, and the two branches both occur in ordinary use — `bash`
    /// turns it on at every prompt and `gh`'s prompts never touch it. Sending the wrapper
    /// unconditionally puts its bytes into a far end that never asked; never sending it runs
    /// each pasted line as it arrives, which is data loss rather than noise.
    ///
    /// Nothing happens while Acter owns the line: there the paste is the edit field's own,
    /// and the browser has already done it.
    fn paste(&self, session: SessionId, text: &str);
}
