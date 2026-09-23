//! Port (driven): the event-emission seam toward the frontend.

use crate::SessionEvent;

/// A closed channel (the webview went away) is not a domain error, so `send` returns nothing.
pub trait EventSink: Send + Sync {
    fn send(&self, event: SessionEvent);
}
