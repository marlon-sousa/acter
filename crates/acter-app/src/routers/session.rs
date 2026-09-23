//! Adapter: the session Tauri routers, each delegating to the `SessionApi` port in managed
//! state.

use std::sync::Arc;

use acter_core::{EventSink, KeyAck, KeyPress, LineOwner, SessionEvent, SessionId, SubmitAck};
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

#[command]
pub(crate) fn send_key(state: State<'_, AppState>, session_id: u32, key: KeyPress) -> KeyAck {
    state.session.send_key(SessionId(session_id), key)
}

#[command]
pub(crate) fn set_line_owner(state: State<'_, AppState>, session_id: u32, owner: LineOwner) {
    state.session.set_line_owner(SessionId(session_id), owner);
}

#[command]
pub(crate) fn paste(state: State<'_, AppState>, session_id: u32, text: String) {
    state.session.paste(SessionId(session_id), &text);
}
