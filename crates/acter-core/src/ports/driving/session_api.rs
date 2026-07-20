//! Port (driving): what the frontend may ask of a session. Services implement it,
//! routers depend on it through `Arc<dyn SessionApi>` — the concrete backend (the A3
//! fake, the real service at convergence) is chosen only in the composition root.

use std::sync::Arc;

use crate::{EventSink, SessionId, SubmitAck};

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
}
