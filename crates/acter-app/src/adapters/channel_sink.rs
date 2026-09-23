//! Adapter: `ChannelSink` implements the `EventSink` driven port over a Tauri IPC Channel.

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
        // A closed channel means the webview reloaded or went away; the next attach restores
        // delivery.
        let _ = self.channel.send(event);
    }
}
