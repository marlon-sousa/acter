//! Port (driving): what the frontend may ask of a session.

use std::sync::Arc;

use crate::{EventSink, KeyAck, KeyPress, LineOwner, SessionId, SubmitAck};

pub trait SessionApi: Send + Sync {
    fn attach_session(&self, session: SessionId, sink: Arc<dyn EventSink>);

    /// Returns at once with the correlation id every later event about this command carries.
    fn submit_command(&self, session: SessionId, line: &str) -> SubmitAck;

    fn send_key(&self, session: SessionId, key: KeyPress) -> KeyAck;

    /// Never inferred from the far end's output.
    fn set_line_owner(&self, session: SessionId, owner: LineOwner);

    /// Does nothing while Acter owns the line, and brackets the text only when the far end
    /// turned bracketed paste on.
    fn paste(&self, session: SessionId, text: &str);
}
