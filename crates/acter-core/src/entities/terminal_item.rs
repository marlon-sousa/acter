//! Entity/value: one element of the ordered stream a terminal engine emits — a line of
//! text with its identity, a shell-integration marker it recognized, or a switch between
//! the normal and alternate screens.
//!
//! Text, not bytes: carriage returns, colour changes and prompt repaints are already
//! resolved by the emulator, which is the stream DESIGN's auto-read threshold was always
//! about (it counts extracted text precisely because escape sequences would inflate a
//! byte count).
//!
//! Lines, not text: a terminal's output is not append-only, however much it looks like
//! one. A progress bar, a spinner, `cargo`'s status line and `docker pull`'s stack of
//! per-layer bars all repaint what they already wrote, on the primary screen, with no
//! alternate screen involved. So every piece of text names the line it belongs to and
//! says what it did to it — DESIGN's Decided "output is a stream of identified lines,
//! and a rewrite is a revision", which spec B3 decision 6 implements.

use crate::{Osc133Marker, Screen};

/// Identifies one line of output for as long as anything may still revise it.
///
/// Opaque and monotonic: the engine mints one when a line is first emitted and never
/// reuses it. Deliberately **not** a grid coordinate — a row index survives scrolling
/// but breaks on resize, on scrollback eviction, and on alt-screen entry, which swaps in
/// a separate grid with its own coordinate space. Ids are session-global and outlive the
/// command block that produced them, so a frontend can find a line whichever block it
/// came from (spec B3, decision 7).
///
/// `u64`, unlike [`CommandId`](crate::CommandId)'s `u32`, because lines are minted per
/// line of output rather than per submitted command. It is not a protocol type yet: B6
/// promotes it when the wire format learns about lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineId(pub u64);

/// What one [`TerminalItem::Line`] did to the line it names.
///
/// Two kinds would be information-sufficient — whole text plus a final flag lets a
/// consumer diff for itself — but the engine already did that diff to detect the
/// rewrite, so re-deriving it downstream would make every consumer keep a copy of every
/// line. Three kinds also keep the common case cheap: [`Appended`](Self::Appended)
/// carries only the delta, so a session containing no rewrites produces exactly the
/// append-only stream that existed before this type did.
///
/// Which path consumes which follows DESIGN's separate-paths decision exactly. The
/// **buffer** applies all three, assigning or appending by id, so it always shows current
/// state. **Speech** takes `Appended` as it always has, ignores `Rewritten` as
/// buffer-only churn, and takes `Settled` as the line's final word — so a spinner is
/// never read mid-spin and its result still is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// cursor addressing, which is exactly how an in-place progress display works
    /// (spec B3, decisions 5 and 6).
    Settled,
}

/// One item from the engine. A batch of these is what
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
    /// It travels in this stream rather than on a side channel because *where* the
    /// switch happened is the whole point: one read from a PTY routinely carries
    /// `ESC[?1049h` followed immediately by the application's first full repaint, which
    /// is what `vim` and `nano` write on startup. Polling the screen after the batch
    /// cannot tell which text preceded the switch, so the repaint would be attributed to
    /// the finished command and spoken as its output (spec B3, decisions 2 and 3).
    ScreenChanged(Screen),
}
