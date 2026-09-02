//! Entity/value: the things a menu item can ask the window to do.
//!
//! **Three, and every one of them opens a dialog the frontend owns.** What the operating
//! system's menu bar can do on its own — quit, hide, copy, minimise — is the platform's and
//! is never named here; what is named here is the part of a menu only Acter can answer, so
//! this enum is exactly the list of items that have to reach the webview (spec M3,
//! decision 5).
//!
//! **It is a protocol type because it crosses the boundary.** A chosen item is emitted to
//! the frontend carrying one of these, and the frontend's switch over it is exhaustive — so
//! a variant added to a menu with no dialog behind it fails to compile rather than reaching
//! a listener as an item that does nothing. That is the rule `ConnectQuestion` already holds
//! the frontend to.

use serde::{Deserialize, Serialize};
use specta::Type;

/// What a menu item Acter answers itself asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub enum MenuAction {
    /// Open the Connect dialog — the same action the button and the Windows menu run.
    Connect,
    /// Open the help topic, at its first section, exactly as F1 does.
    Help,
    /// Open the About dialog.
    About,
}
