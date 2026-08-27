//! Adapter: `ConnectSteps` implements the `ConnectSink` driven port over a Tauri IPC
//! Channel — what a connection says while it is being made, on its way to the window.
//!
//! **A Channel rather than a broadcast event**, for the reason `attach_session` uses one:
//! it is ordered and bound to the caller who started this attempt. Two windows connecting
//! at once must not hear each other's questions, and a host-key question arriving before
//! the progress line that preceded it would be a conversation out of order.
//!
//! The second Channel in the app, and the reason there are now two: that one carries a
//! *session's* events and only exists once there is a session, and this one carries the
//! steps of there coming to be one (ARCHITECTURE, IPC rules).

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
        // A closed channel means the webview reloaded or the window went away while a
        // connection was being made. Nothing here can do anything about it: the attempt
        // will end on its own terms, and any question it asks will go unanswered and be
        // read as giving up — which is the safe reading (spec B9, decision 3).
        let _ = self.channel.send(step);
    }
}
