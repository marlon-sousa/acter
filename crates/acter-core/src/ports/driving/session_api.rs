//! Port (driving): what the frontend may ask of a session. Services implement it,
//! routers depend on it through `Arc<dyn SessionApi>` — which concrete backend, and
//! which transport under it, is chosen only in the composition root.

use std::sync::Arc;

use crate::{EventSink, KeyAck, KeyPress, SessionId, SubmitAck};

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
}
