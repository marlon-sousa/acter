//! Adapter (internal to the terminal-engine adapter): the event listener that collects
//! device-query replies.

use std::mem::take;
use std::sync::{Arc, Mutex, MutexGuard};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::vte::ansi::{NamedColor, Rgb};

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

    pub(super) fn take(&self) -> Vec<u8> {
        take(&mut self.state().pending)
    }

    pub(super) fn resized(&self, columns: u16, screen_lines: u16) {
        let mut state = self.state();
        state.columns = columns;
        state.screen_lines = screen_lines;
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl EventListener for DeviceReplies {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => self.state().pending.extend_from_slice(text.as_bytes()),
            Event::ColorRequest(index, format) => {
                let reply = format(default_color(index));
                self.state().pending.extend_from_slice(reply.as_bytes());
            }
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
