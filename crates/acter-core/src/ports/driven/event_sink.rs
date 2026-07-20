//! Port (driven): the event-emission seam. The domain streams `SessionEvent`s to the
//! frontend through this trait; the production adapter wraps a Tauri Channel, tests
//! inject a recording fake. It is the seam the whole event pipeline is exercised
//! through, so it stays as small as one method.

use crate::SessionEvent;

/// Emits one protocol event toward the frontend. `Send + Sync` because events are
/// produced from session/backend tasks on other threads. Fire-and-forget: a closed
/// channel (the webview went away) is not a domain error, so `send` returns nothing.
pub trait EventSink: Send + Sync {
    fn send(&self, event: SessionEvent);
}
