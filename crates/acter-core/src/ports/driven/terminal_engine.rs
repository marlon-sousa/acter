//! Port (driven): the terminal emulation engine — bytes in, one ordered stream of
//! identified lines, recognized shell-integration markers and screen transitions out.
//!
//! The adapter behind it (`acter-term`) runs a real emulation core, which is DESIGN's
//! Decided phasing item: the non-interactive text view is derived from the grid, never
//! from regex-stripping escape sequences out of the raw byte stream.

use crate::{Screen, TerminalItem};

/// One session's emulator. Driven entirely by byte slices, so a session actor can hand
/// it whatever a read produced and get back everything the domain needs to know about
/// it — no I/O, no clock, no runtime.
pub trait TerminalEngine {
    /// Feeds one read's worth of bytes and returns what they meant, in stream order.
    ///
    /// Batch in, batch out, the same shape
    /// [`BoundaryTracker::observe`](crate::BoundaryTracker::observe) already takes. State
    /// carries across calls: an escape sequence split across two reads is resumed, not
    /// lost, which is DESIGN's reliability case.
    fn advance(&mut self, bytes: &[u8]) -> Vec<TerminalItem>;

    /// Which screen is current, from the emulator's own mode.
    ///
    /// The authority on *what* is on screen; [`TerminalItem::ScreenChanged`] is the
    /// authority on *where in the stream* it changed. Both exist because a single read
    /// routinely carries the switch and the new screen's first repaint together.
    fn screen(&self) -> Screen;

    /// Resizes the emulated screen. Declared now because the IPC protocol already
    /// defines a resize and a local PTY needs dimensions at creation.
    fn resize(&mut self, columns: u16, screen_lines: u16);

    /// Takes the device-query answers the caller must write back to the transport,
    /// draining them so they are never sent twice.
    ///
    /// This exists because an emulator does not answer device queries itself: it asks
    /// its embedder to. Drop those answers and a program that queries the terminal —
    /// for a device attributes report, a cursor position, a colour — waits forever, and
    /// for this product that surfaces as a session that has simply gone quiet, with
    /// nothing to announce and no way for the user to tell why (spec B3, decision 4).
    ///
    /// Writing the bytes is the caller's job, so nothing consumes this until `Transport`
    /// lands in B3.5.
    fn take_replies(&mut self) -> Vec<u8>;

    /// Where the far end's cursor is, and whether it is using one.
    ///
    /// **What places the caret in the far-end line, which is why left, right, Home and End
    /// need no speech path at all** (spec 28, decision 5). Those keys rewrite no text and
    /// are invisible to every rule that watches rows change; the answer to them is a caret
    /// rather than a sentence, and a caret needs a column.
    fn cursor(&self) -> Cursor;

    /// The two modes the far end can turn on that change what Acter must send it.
    ///
    /// One accessor for two facts the emulator already tracks, because they settle the same
    /// kind of question: which spelling an arrow key is, and whether a paste may be
    /// bracketed (spec 28, decision 5).
    fn modes(&self) -> TerminalModes;
}

/// Where the far end's cursor is on the screen it is drawing.
///
/// Grid coordinates rather than a [`LineId`](crate::LineId): the line the cursor is on is
/// something the caller already knows from the stream, and the position within the row is
/// what this adds. Both numbers are zero-based, counted from the top-left of the screen
/// area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// How many cells from the left of the screen the cursor sits.
    pub column: u16,
    /// Which row of the screen area it sits on.
    ///
    /// Read to tell "the caret moved along this row" from "the cursor went somewhere else
    /// entirely", which are two different answers to a key.
    pub row: u16,
    /// Whether the far end is showing it.
    ///
    /// **It earns its place because `gh` hides the cursor and parks it off the list**
    /// (measured 2026-09-02): it writes `ESC[?25l` before drawing, repaints from
    /// `ESC[2;1H`, and leaves the cursor on the blank row below its options until the
    /// prompt is answered. A caret placed from a cursor the far end is not using would put
    /// a listener somewhere the far end never went.
    pub visible: bool,
}

/// The modes the far end has turned on that Acter has to honour.
///
/// Two rather than every mode the emulator tracks, because these are the two that change
/// what Acter *sends*. Everything else a terminal mode does is about what the grid holds,
/// which the grid already answers by holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalModes {
    /// DECCKM: the far end has asked for `ESC O A` rather than `ESC [ A`.
    ///
    /// `readline`-driven shells and most full-screen programs turn it on the moment they
    /// take the keyboard — though `bash` under WSL, measured 2026-08-31, never does, and
    /// answers both spellings because it binds both.
    pub application_cursor_keys: bool,
    /// The far end has asked for pasted text to arrive wrapped in `ESC[200~` and `ESC[201~`.
    ///
    /// `bash` turns it on at every prompt and clears it on submission; `gh`'s prompts never
    /// touch it. Both branches occur in ordinary use, which is why a paste asks rather than
    /// assuming: the wrapper sent to a far end that never asked puts its own bytes into the
    /// line, and never sending it runs each pasted line as it arrives (spec 28,
    /// decision 10).
    pub bracketed_paste: bool,
}
