//! Adapter: `ConnectSteps` implements the `ConnectSink` driven port over a Tauri IPC
//! Channel bound to the window that started the attempt.

use acter_core::{ConnectSink, ConnectStep};
use tauri::ipc::Channel;

pub(crate) struct ConnectSteps {
    channel: Channel<ConnectStep>,
}

impl ConnectSteps {
    pub(crate) fn new(channel: Channel<ConnectStep>) -> Self {
        Self { channel }
    }
}

impl ConnectSink for ConnectSteps {
    fn send(&self, step: ConnectStep) {
        // A closed channel leaves any question the attempt then asks unanswered, and the
        // attempt waits on it indefinitely.
        let _ = self.channel.send(step);
    }
}
