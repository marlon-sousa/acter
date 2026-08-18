//! Adapter (internal to the terminal-engine adapter): the event listener that collects
//! device-query replies.
//!
//! The emulator never answers a device query itself. It asks its listener to, emitting a
//! write request for device attributes, cursor-position and status reports, and two
//! requests that carry a formatter closure — a colour query and a text-area size query —
//! which must be called to produce the reply. Discard them and a program that queries the
//! terminal and waits for the answer waits forever, which for this product surfaces as a
//! session that has simply gone quiet, with nothing to announce and no way for the user
//! to tell why (spec B3, decision 4).
//!
//! So this type captures the bytes and the engine hands them out; writing them to the
//! transport is the caller's job, which arrives with `Transport` in B3.5.

use std::mem::take;
use std::sync::{Arc, Mutex, MutexGuard};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::vte::ansi::{NamedColor, Rgb};

/// A handle on one session's reply buffer. Cloned rather than borrowed: the emulator
/// owns its listener outright, and the listener is handed events through a shared
/// reference, so the buffer has to live behind shared ownership either way.
#[derive(Debug, Clone)]
pub(super) struct DeviceReplies(Arc<Mutex<State>>);

#[derive(Debug)]
struct State {
    pending: Vec<u8>,
    columns: u16,
    screen_lines: u16,
}

impl DeviceReplies {
    pub(super) fn new(columns: u16, screen_lines: u16) -> Self {
        Self(Arc::new(Mutex::new(State {
            pending: Vec::new(),
            columns,
            screen_lines,
        })))
    }

    /// Takes everything captured so far, leaving the buffer empty so no answer is ever
    /// written twice.
    pub(super) fn take(&self) -> Vec<u8> {
        take(&mut self.state().pending)
    }

    /// Keeps the dimensions a size query is answered from in step with the emulator's.
    pub(super) fn resized(&self, columns: u16, screen_lines: u16) {
        let mut state = self.state();
        state.columns = columns;
        state.screen_lines = screen_lines;
    }

    /// A poisoned lock would mean a panic while a reply was being appended, which
    /// nothing here can do; recovering the buffer is still better than taking a session
    /// down over it.
    fn state(&self) -> MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl EventListener for DeviceReplies {
    fn send_event(&self, event: Event) {
        match event {
            // Device attributes, cursor position, status reports, the size-in-characters
            // report: the emulator has already formatted these.
            Event::PtyWrite(text) => self.state().pending.extend_from_slice(text.as_bytes()),
            // A colour query. This terminal renders no colours at all, so it answers with
            // a monochrome default rather than leaving the program waiting.
            Event::ColorRequest(index, format) => {
                let reply = format(default_color(index));
                self.state().pending.extend_from_slice(reply.as_bytes());
            }
            // A size-in-pixels query. There is no rendering surface, so there is no cell
            // size to report and the honest answer is zero; the row and column counts are
            // the dimensions this engine was built with.
            Event::TextAreaSizeRequest(format) => {
                let size = {
                    let state = self.state();
                    WindowSize {
                        num_lines: state.screen_lines,
                        num_cols: state.columns,
                        cell_width: 0,
                        cell_height: 0,
                    }
                };
                let reply = format(size);
                self.state().pending.extend_from_slice(reply.as_bytes());
            }
            // Captured and dropped, with their eventual owners named. The bell is
            // DESIGN's beep, which is a frontend view adapter, and the window title is
            // not phase 1. Clipboard events cannot arise at all: OSC 52 is configured
            // off, which is also the right default for a terminal with no clipboard
            // story yet. The rest are renderer concerns this engine has no renderer for.
            Event::Bell
            | Event::Title(_)
            | Event::ResetTitle
            | Event::ClipboardStore(..)
            | Event::ClipboardLoad(..)
            | Event::MouseCursorDirty
            | Event::CursorBlinkingChange
            | Event::Wakeup
            | Event::Exit
            | Event::ChildExit(_) => {}
        }
    }
}

/// Monochrome stand-ins for a palette this terminal does not have: black where the
/// background was asked for, white everywhere else.
fn default_color(index: usize) -> Rgb {
    if index == NamedColor::Background as usize {
        Rgb { r: 0, g: 0, b: 0 }
    } else {
        Rgb {
            r: 0xff,
            g: 0xff,
            b: 0xff,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_request_is_captured_and_drained_once() {
        let replies = DeviceReplies::new(80, 24);
        replies.send_event(Event::PtyWrite("\x1b[3;7R".to_owned()));

        assert_eq!(replies.take(), b"\x1b[3;7R");
        assert!(replies.take().is_empty());
    }

    #[test]
    fn a_size_query_is_answered_from_the_current_dimensions() {
        let replies = DeviceReplies::new(80, 24);
        replies.resized(100, 30);
        replies.send_event(Event::TextAreaSizeRequest(Arc::new(|size| {
            format!("{}x{}", size.num_cols, size.num_lines)
        })));

        assert_eq!(replies.take(), b"100x30");
    }

    #[test]
    fn a_colour_query_is_answered_rather_than_left_hanging() {
        let replies = DeviceReplies::new(80, 24);
        replies.send_event(Event::ColorRequest(
            NamedColor::Background as usize,
            Arc::new(|color| format!("{},{},{}", color.r, color.g, color.b)),
        ));

        assert_eq!(replies.take(), b"0,0,0");
    }

    #[test]
    fn a_bell_leaves_nothing_to_write_back() {
        let replies = DeviceReplies::new(80, 24);
        replies.send_event(Event::Bell);

        assert!(replies.take().is_empty());
    }
}
