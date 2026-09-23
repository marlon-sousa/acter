//! Entity/value: one element of the ordered stream a terminal engine emits — a line of
//! text with its identity, a shell-integration marker it recognized, or a switch between
//! the normal and alternate screens.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Osc133Marker, Screen};

/// Minted once per line, never reused, unique across the session; not a grid row.
///
/// Exported as `u32` because `specta-typescript` refuses `u64`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type,
)]
#[specta(type = u32)]
pub struct LineId(pub u64);

/// Speech reads `Appended` and `Settled` and never `Rewritten`, so a spinner is not read
/// mid-spin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum LineRevision {
    /// The text is only what was added to the end of the line.
    Appended,
    /// The text is the whole line.
    Rewritten,
    /// The text is the whole line, which can no longer change and emits nothing further.
    /// Sent when it leaves the screen area, its block closes, the screen switches or the
    /// terminal resizes, never at a newline.
    Settled,
}

/// A batch of these is what
/// [`BoundaryTracker::observe`](crate::BoundaryTracker::observe) takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalItem {
    Line {
        id: LineId,
        text: String,
        revision: LineRevision,
    },
    Marker(Osc133Marker),
    /// In the stream, between the lines before and after the switch; see
    /// acter-term's alacritty_engine.rs.
    ScreenChanged(Screen),
}
