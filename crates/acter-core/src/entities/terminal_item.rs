//! Entity/value: one element of the ordered stream a terminal engine emits — a line of
//! text with its identity, a shell-integration marker it recognized, or a switch between
//! the normal and alternate screens.
//!
//! Text, not bytes: the emulator already resolves carriage returns, colour changes and
//! prompt repaints, and the auto-read threshold counts extracted text so escape sequences
//! never inflate it.
//!
//! Lines, not text: a terminal's output is not append-only. A progress bar, a spinner,
//! `cargo`'s status line and `docker pull`'s stack of per-layer bars all repaint what
//! they already wrote, on the primary screen, with no alternate screen involved. So every
//! piece of text names the line it belongs to and what it did to it.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Osc133Marker, Screen};

/// Opaque and monotonic: the engine mints one when a line is first emitted and never
/// reuses it. Deliberately not a grid coordinate — a row index survives scrolling but
/// breaks on resize, on scrollback eviction, and on alt-screen entry, which swaps in a
/// separate grid with its own coordinate space. Ids are session-global and outlive the
/// command block that produced them, so a frontend can find a line whichever block it
/// came from.
///
/// `u64`, unlike [`CommandId`](crate::CommandId)'s `u32`, because lines are minted per
/// line of output rather than per submitted command.
///
/// Without an id to apply a revision to, arrowing a history list would append a line per
/// press, and a cancelled prompt would leave its option rows behind after the far end
/// itself blanked them.
///
/// Exported to TypeScript as a plain number by naming a narrower integer to specta:
/// `specta-typescript` refuses `u64` outright, to stop a caller silently losing precision
/// in a JSON number. The annotation is a statement about the exported shape, not about
/// the id.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type,
)]
#[specta(type = u32)]
pub struct LineId(pub u64);

/// [`Appended`](Self::Appended) carries only the delta, since the engine already diffed
/// the line to detect the rewrite; a session containing no rewrites produces exactly the
/// append-only stream that existed before this type did.
///
/// The buffer applies all three, assigning or appending by id, so it always shows
/// current state. Speech takes `Appended` as it always has, ignores `Rewritten` as
/// buffer-only churn, and takes `Settled` as the line's final word, so a spinner is
/// never read mid-spin and its result still is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum LineRevision {
    /// The text is the delta added to the end of the line. The ordinary case: output
    /// streaming in, including the very first text a line ever carries.
    Appended,
    /// The line changed below what was already emitted; the text is the whole line.
    Rewritten,
    /// The line can no longer change; the text is its final content.
    ///
    /// Every line settles at most once, and nothing follows a line's settlement. A line
    /// settles when change has become impossible: it scrolled out of the active screen
    /// area, its command block closed, the screen changed, or the terminal was resized.
    /// Not at a newline — until a row leaves the screen area it stays reachable by
    /// cursor addressing, which is exactly how an in-place progress display works.
    Settled,
}

/// A batch of these is what
/// [`BoundaryTracker::observe`](crate::BoundaryTracker::observe) takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalItem {
    /// Text belonging to an identified line, and what it did to that line.
    Line {
        id: LineId,
        text: String,
        revision: LineRevision,
    },
    /// A shell-integration marker the engine recognized.
    Marker(Osc133Marker),
    /// The emulator switched between the normal and alternate screens.
    ///
    /// Travels in this stream rather than on a side channel because where the switch
    /// happened is the whole point: one read from a PTY routinely carries
    /// `ESC[?1049h` followed immediately by the application's first full repaint, which
    /// is what `vim` and `nano` write on startup. Polling the screen after the batch
    /// cannot tell which text preceded the switch, so the repaint would be attributed to
    /// the finished command and spoken as its output.
    ScreenChanged(Screen),
}
