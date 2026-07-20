//! Adapter: the session Tauri routers — one-line `#[tauri::command]` functions that
//! delegate to the `SessionApi` port from managed state. `attach_session` wraps the
//! JS Channel in a `ChannelSink`; `submit_command` passes primitive args straight
//! through and returns the ack. Tauri-shaped signatures stop here.

use std::sync::Arc;

use acter_core::{EventSink, SessionEvent, SessionId, SubmitAck};
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
