//! Adapter: the connect Tauri routers — the named actions of `ConnectApi`, each a
//! one-line `#[tauri::command]` delegating to the port in managed state.
//!
//! **These are the whole of "connecting" as far as the framework is concerned**, which is
//! the point of B7's shape: a menu item, a dialog and a `--connect` switch are all callers
//! of the same actions, and a test calls them directly with no window and no webview. What
//! stays untestable is only whether a menu *widget* exists and fires, which the NVDA pass
//! observes.
//!
//! **Nothing here carries a key and a value** (spec 26, decision 11). The frontend never
//! reads or writes the settings document: the saved connections arrive as typed rows and
//! change through named actions, each answering the sentence a listener hears. A stringly
//! surface would give up the one guarantee that makes a new setting a compile error rather
//! than a silent nothing.
//!
//! **Since B9, starting a connection does not answer with one** (spec B9). Connecting can
//! stop partway to ask a person about a host key or a password, so `use_profile` answers
//! with the id of the *attempt* and everything after that arrives on the Channel the caller
//! passed: progress, questions, and finally a session or a sentence saying why not.
//!
//! The reason is not tidiness. A synchronous `#[tauri::command]` runs on the main thread,
//! so a command that waited for a dialog would hold the thread that the answering command
//! needs in order to be dispatched — a deadlock exactly when the dialog appears. Every
//! router here returns at once.

use std::sync::Arc;

use acter_core::{
    AttemptId, ConnectAnswer, ConnectSink, Connectable, Connected, LaunchRequest, ProfileId,
    SavedConnections, SetUp,
};
use tauri::ipc::Channel;
use tauri::{State, command};

use crate::adapters::ConnectSteps;
use crate::container::AppState;

/// Everything this machine offers, asked of the machine now rather than remembered from
/// startup: a distribution installed while Acter is open appears the next time the list is
/// opened, without a restart.
#[command]
pub(crate) fn connectable(state: State<'_, AppState>) -> Vec<Connectable> {
    state.connect.connectable()
}

/// Start this profile, and report what happens on `steps`.
///
/// **`set_up` is the Connect dialog's checkbox** (spec B9.5, decision 9): whether this
/// connection may run one command inside the session once it is established. It travels with
/// the attempt rather than being stored, because there is no profile store to keep it in until
/// B8 (decision 10).
///
/// Answers with the attempt's id, which is what an answering invoke carries back. The
/// session, when there is one, arrives as the `Arrived` step; the caller then attaches to
/// its `session`, which is deliberately a second call — the frontend's chance to clear a
/// buffer still holding the previous shell's output before any of the new one's arrives
/// (spec B7, decision 1).
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

/// What the person decided about whatever this attempt last asked.
///
/// **This is the invoke a password arrives on, and it goes nowhere else.** It is not part
/// of the session surface the debug event recorder wraps (spec A3.2), so a password cannot
/// reach a debug tape; and `ConnectAnswer` derives no `Serialize`, so nothing can put one
/// back on the wire (spec B9, decision 4).
#[command]
pub(crate) fn answer_connect(
    state: State<'_, AppState>,
    attempt: AttemptId,
    answer: ConnectAnswer,
) {
    state.connecting.answer(attempt, answer);
}

/// This attempt is over as far as the window is concerned, so it can be forgotten.
#[command]
pub(crate) fn attempt_ended(state: State<'_, AppState>, attempt: AttemptId) {
    state.connecting.ended(attempt);
}

/// Which far end this window is on, or `null` for a window connected to nothing.
///
/// **Replaces A9's `connection`, which read the environment variable the launch was given.**
/// That was honest while a launch was the only way to have a session; now that a session can
/// be replaced while the window is open, the only answer that stays true is the one the
/// service gives — and it is the connect list's own label, so what a user chose and what the
/// window then calls itself are the same words.
#[command]
pub(crate) fn connected(state: State<'_, AppState>) -> Option<Connected> {
    state.connect.connected()
}

/// Every saved connection, freshly read and resolved against what this machine has now
/// (spec 26, decision 11).
#[command]
pub(crate) fn saved(state: State<'_, AppState>) -> SavedConnections {
    state.connect.saved()
}

/// Write the live session down under this name, and answer the sentence to say.
///
/// **A `Result`, because both halves are sentences a listener hears**: "Saved as X." on the
/// way out, and the name rule or a collision on the way back (decisions 8 and 11). Tauri
/// renders the error half as a rejected promise, which is what the dialog already handles
/// for a connection that could not be made.
#[command]
pub(crate) fn save_connection(state: State<'_, AppState>, name: String) -> Result<String, String> {
    state.connect.save_connection(&name)
}

/// Give a saved connection a different name.
#[command]
pub(crate) fn rename_connection(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<String, String> {
    state.connect.rename_connection(&from, &to)
}

/// Remove one — the one thing here nobody can undo, which is why the dialog asks first
/// (decision 15).
#[command]
pub(crate) fn forget_connection(
    state: State<'_, AppState>,
    name: String,
) -> Result<String, String> {
    state.connect.forget_connection(&name)
}

/// Whether a new connection that has just come up should offer to save itself
/// (decision 19).
///
/// **Asked at the moment of the offer rather than at startup**, so a preference set in
/// another window is honoured without a restart — `connectable`'s rule again.
#[command]
pub(crate) fn offer_to_save(state: State<'_, AppState>) -> bool {
    state.connect.offer_to_save()
}

/// Record that it should not, which is the offer's own checkbox.
#[command]
pub(crate) fn stop_offering_to_save(state: State<'_, AppState>) -> Result<(), String> {
    state.connect.stop_offering_to_save()
}

/// What `acter --connect <name>` asked for, or `null` for an ordinary launch (spec 26,
/// decision 20).
///
/// **Asked rather than acted on.** A saved SSH connection has to ask about a host key and
/// then for a password, and there is nobody to ask until there is a window — so the switch
/// becomes a request the window carries out through the same call the Connect dialog makes.
#[command]
pub(crate) fn requested_at_launch(state: State<'_, AppState>) -> Option<LaunchRequest> {
    state.connect.requested_at_launch()
}
