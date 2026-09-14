//! Entity/value: the things a menu item can ask the window to do.
//!
//! A chosen item is emitted to the frontend, whose switch over it is exhaustive: a
//! variant with no dialog behind it fails to compile there rather than reaching a
//! listener as an item that does nothing.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub enum MenuAction {
    /// Opens the list of saved connection names.
    Connect,
    /// Opens the list of connection kinds.
    NewConnection,
    /// Unconnected, opens no dialog and reports there is nothing to save.
    SaveConnection,
    /// Opens the help topic at its first section, as F1 does.
    Help,
    About,
}
