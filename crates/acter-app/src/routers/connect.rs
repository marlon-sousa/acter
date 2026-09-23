//! Adapter: the connect Tauri routers, each delegating to `ConnectApi` in managed state.

use std::sync::Arc;

use acter_core::{
    AttemptId, ConnectAnswer, ConnectSink, Connectable, Connected, LaunchRequest, ProfileId,
    SavedConnections, SetUp,
};
use tauri::ipc::Channel;
use tauri::{State, command};

use crate::adapters::ConnectSteps;
use crate::container::AppState;

#[command]
pub(crate) fn connectable(state: State<'_, AppState>) -> Vec<Connectable> {
    state.connect.connectable()
}

#[command]
pub(crate) fn use_profile(
    state: State<'_, AppState>,
    profile: ProfileId,
    set_up: SetUp,
    origin: Option<String>,
    steps: Channel<acter_core::ConnectStep>,
) -> AttemptId {
    state.connecting.begin(
        profile,
        set_up,
        origin,
        Arc::new(ConnectSteps::new(steps)) as Arc<dyn ConnectSink>,
    )
}

/// Carries a password, so it must stay off the session surface the debug recorder wraps.
#[command]
pub(crate) fn answer_connect(
    state: State<'_, AppState>,
    attempt: AttemptId,
    answer: ConnectAnswer,
) {
    state.connecting.answer(attempt, answer);
}

#[command]
pub(crate) fn attempt_ended(state: State<'_, AppState>, attempt: AttemptId) {
    state.connecting.ended(attempt);
}

/// `None` means the window is connected to nothing.
#[command]
pub(crate) fn connected(state: State<'_, AppState>) -> Option<Connected> {
    state.connect.connected()
}

#[command]
pub(crate) fn saved(state: State<'_, AppState>) -> SavedConnections {
    state.connect.saved()
}

/// Both halves are sentences a listener hears; Tauri renders `Err` as a rejected promise.
#[command]
pub(crate) fn save_connection(state: State<'_, AppState>, name: String) -> Result<String, String> {
    state.connect.save_connection(&name)
}

#[command]
pub(crate) fn rename_connection(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<String, String> {
    state.connect.rename_connection(&from, &to)
}

#[command]
pub(crate) fn forget_connection(
    state: State<'_, AppState>,
    name: String,
) -> Result<String, String> {
    state.connect.forget_connection(&name)
}

#[command]
pub(crate) fn offer_to_save(state: State<'_, AppState>) -> bool {
    state.connect.offer_to_save()
}

#[command]
pub(crate) fn stop_offering_to_save(state: State<'_, AppState>) -> Result<(), String> {
    state.connect.stop_offering_to_save()
}

/// `None` means an ordinary launch with no `--connect <name>`.
#[command]
pub(crate) fn requested_at_launch(state: State<'_, AppState>) -> Option<LaunchRequest> {
    state.connect.requested_at_launch()
}
