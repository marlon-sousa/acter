//! Adapter: `ChannelSink` implements the `EventSink` driven port over a Tauri IPC
//! Channel — the only place in the app a Channel is held (ARCHITECTURE, IPC rules).
//! The frontend creates a JS `Channel<SessionEvent>` and passes it in the
//! `attach_session` invoke; the session's event stream flows down it.

use acter_core::{EventSink, SessionEvent};
use tauri::ipc::Channel;

pub(crate) struct ChannelSink {
    channel: Channel<SessionEvent>,
}

impl ChannelSink {
    pub(crate) fn new(channel: Channel<SessionEvent>) -> Self {
        Self { channel }
    }
}

impl EventSink for ChannelSink {
    fn send(&self, event: SessionEvent) {
        // A closed channel (the webview reloaded or went away) is not a domain error;
        // the next attach re-establishes delivery. Drop the send silently.
        let _ = self.channel.send(event);
    }
}
