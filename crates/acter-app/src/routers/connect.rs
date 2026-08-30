//! Adapter: the connect Tauri routers — the three named actions of `ConnectApi`, each a
//! one-line `#[tauri::command]` delegating to the port in managed state.
//!
//! **These are the whole of "connecting" as far as the framework is concerned**, which is
//! the point of B7's shape: a menu item, a dialog and a `--profile` switch are all callers
//! of the same three, and a test calls them directly with no window and no webview. What
//! stays untestable is only whether a menu *widget* exists and fires, which the NVDA pass
//! observes.
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

use acter_core::{AttemptId, ConnectAnswer, ConnectSink, Connectable, Connected, ProfileId, SetUp};
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
    steps: Channel<acter_core::ConnectStep>,
) -> AttemptId {
    state.connecting.begin(
        profile,
        set_up,
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
