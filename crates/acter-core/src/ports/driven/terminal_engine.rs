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
}
