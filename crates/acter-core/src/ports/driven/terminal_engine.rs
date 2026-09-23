//! Port (driven): the terminal emulation engine — bytes in, one ordered stream of
//! identified lines, recognized shell-integration markers and screen transitions out.

use crate::{Screen, TerminalItem};

pub trait TerminalEngine {
    /// State carries across calls: an escape sequence split across two reads is resumed,
    /// not lost.
    fn advance(&mut self, bytes: &[u8]) -> Vec<TerminalItem>;

    fn screen(&self) -> Screen;

    fn resize(&mut self, columns: u16, screen_lines: u16);

    /// The caller must write these to the transport, or a program that queried the
    /// terminal waits forever.
    fn take_replies(&mut self) -> Vec<u8>;

    fn cursor(&self) -> Cursor;

    fn modes(&self) -> TerminalModes;
}

/// Zero-based, counted from the top-left of the screen area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub column: u16,
    pub row: u16,
    /// `gh` hides the cursor and parks it below its options, so a caret must not follow a
    /// cursor that is not visible.
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalModes {
    /// DECCKM: arrows are sent as `ESC O A` rather than `ESC [ A`.
    pub application_cursor_keys: bool,
    pub bracketed_paste: bool,
}
