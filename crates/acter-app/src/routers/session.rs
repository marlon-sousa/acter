//! Adapter: the session Tauri routers — one-line `#[tauri::command]` functions that
//! delegate to the `SessionApi` port from managed state. `attach_session` wraps the
//! JS Channel in a `ChannelSink`; `submit_command` and `send_key` pass their arguments
//! straight through and return the ack. Tauri-shaped signatures stop here.

use std::sync::Arc;

use acter_core::{EventSink, KeyAck, KeyPress, SessionEvent, SessionId, SubmitAck};
use tauri::ipc::Channel;
use tauri::{State, command};

use crate::adapters::ChannelSink;
use crate::container::AppState;

#[command]
pub(crate) fn attach_session(
    state: State<'_, AppState>,
    session_id: u32,
    channel: Channel<SessionEvent>,
) {
    let sink: Arc<dyn EventSink> = Arc::new(ChannelSink::new(channel));
    state.session.attach_session(SessionId(session_id), sink);
}

#[command]
pub(crate) fn submit_command(
    state: State<'_, AppState>,
    session_id: u32,
    line: String,
) -> SubmitAck {
    state.session.submit_command(SessionId(session_id), &line)
}

/// A keystroke the frontend did not consume. What it *means* is the domain's
/// (`policies::keybindings`), which is why this carries the key and not an intent.
#[command]
pub(crate) fn send_key(state: State<'_, AppState>, session_id: u32, key: KeyPress) -> KeyAck {
    state.session.send_key(SessionId(session_id), key)
}
