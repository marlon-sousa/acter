//! Adapter: the connect Tauri routers — the three named actions of `ConnectApi`, each a
//! one-line `#[tauri::command]` delegating to the port in managed state.
//!
//! **These are the whole of "connecting" as far as the framework is concerned**, which is
//! the point of B7's shape: a menu item, a dialog and a `--profile` switch are all callers
//! of the same three, and a test calls them directly with no window and no webview. What
//! stays untestable is only whether a menu *widget* exists and fires, which the NVDA pass
//! observes.
//!
//! `use_profile` is the one router in this crate that returns a `Result`. Tauri rejects the
//! frontend's promise with the `Err` string, which is a whole spoken sentence — a failure to
//! connect is read to somebody who has just chosen something from a list and is waiting.

use acter_core::{Connectable, Connected, ProfileId};
use tauri::{State, command};

use crate::container::AppState;

/// Everything this machine offers, asked of the machine now rather than remembered from
/// startup: a distribution installed while Acter is open appears the next time the list is
/// opened, without a restart.
#[command]
pub(crate) fn connectable(state: State<'_, AppState>) -> Vec<Connectable> {
    state.connect.connectable()
}

/// Start this profile and replace whatever was running.
///
/// The caller then attaches to [`Connected::session`], which is deliberately a second call:
/// it is the frontend's chance to clear a buffer still holding the previous shell's output
/// before any of the new one's arrives (spec B7, decision 1).
#[command]
pub(crate) fn use_profile(
    state: State<'_, AppState>,
    profile: ProfileId,
) -> Result<Connected, String> {
    state.connect.use_profile(&profile)
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
