//! Adapter (internal to the terminal-engine adapter): the stream-position sniffer.
//!
//! Every `Handler` method has a default no-op body, so a type that forwarded to `Term` would
//! silently stop forwarding any method a vte release adds, as vte 0.13.0 did with
//! `set_private_mode`; this type forwards nothing.

use std::str::from_utf8;

use acter_core::{ExitCode, Osc133Marker, Screen};
use alacritty_terminal::vte::ansi::{Handler, NamedPrivateMode, PrivateMode};

/// Something the sniffer noticed at the current point in the byte stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Signal {
    Marker(Osc133Marker),
    ScreenChanged(Screen),
}

#[derive(Debug, Default)]
pub(super) struct Sniffer {
    signals: Vec<Signal>,
}

impl Sniffer {
    pub(super) fn signalled(&self) -> bool {
        !self.signals.is_empty()
    }

    pub(super) fn drain(&mut self) -> Vec<Signal> {
        std::mem::take(&mut self.signals)
    }
}

impl Handler for Sniffer {
    fn unhandled_osc(&mut self, params: &[&[u8]]) {
        if let Some(marker) = parse_osc133(params) {
            self.signals.push(Signal::Marker(marker));
        }
    }

    fn set_private_mode(&mut self, mode: PrivateMode) {
        if swaps_screen(mode) {
            self.signals.push(Signal::ScreenChanged(Screen::Alternate));
        }
    }

    fn unset_private_mode(&mut self, mode: PrivateMode) {
        if swaps_screen(mode) {
            self.signals.push(Signal::ScreenChanged(Screen::Normal));
        }
    }
}

/// alacritty_terminal 0.26 also leaves the alternate screen on a full reset (`ESC c`), which
/// calls neither private-mode method, so that switch is never signalled.
fn swaps_screen(mode: PrivateMode) -> bool {
    matches!(
        mode,
        PrivateMode::Named(NamedPrivateMode::SwapScreenAndSetRestoreCursor)
    )
}

/// `None` for any OSC other than a known 133 marker.
fn parse_osc133(params: &[&[u8]]) -> Option<Osc133Marker> {
    if params.first().copied() != Some(b"133".as_slice()) {
        return None;
    }

    match params.get(1).copied()? {
        b"A" => Some(Osc133Marker::PromptStart),
        b"B" => Some(Osc133Marker::CommandStart),
        b"C" => Some(Osc133Marker::OutputStart),
        b"D" => Some(Osc133Marker::CommandEnd(exit_code(params.get(2).copied()))),
        _ => None,
    }
}

fn exit_code(param: Option<&[u8]>) -> Option<ExitCode> {
    let text = from_utf8(param?).ok()?;
    text.parse().ok().map(ExitCode)
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    use super::*;

    fn sniff(bytes: &[u8]) -> Vec<Signal> {
        let mut parser = Processor::<StdSyncHandler>::new();
        let mut sniffer = Sniffer::default();
        parser.advance(&mut sniffer, bytes);
        sniffer.drain()
    }

    #[derive(Default)]
    struct Bells(usize);

    impl Handler for Bells {
        fn bell(&mut self) {
            self.0 += 1;
        }
    }

    fn bells(bytes: &[u8]) -> usize {
        let mut parser = Processor::<StdSyncHandler>::new();
        let mut counted = Bells::default();
        parser.advance(&mut counted, bytes);
        counted.0
    }

    #[test]
    fn a_bell_that_ends_a_marker_never_rings() {
        assert_eq!(
            bells(b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;0\x07"),
            0,
            "every marker of the cycle, bell-terminated, and not one bell"
        );
        assert_eq!(
            bells(b"\x07"),
            1,
            "and a bell the far end actually rang still arrives, so a beep has something to \
             hook"
        );
    }

    #[test]
    fn the_two_spellings_of_the_terminator_are_the_same_markers() {
        assert_eq!(
            sniff(b"\x1b]133;A\x07\x1b]133;B\x07"),
            sniff(b"\x1b]133;A\x1b\\\x1b]133;B\x1b\\")
        );
    }

    #[test]
    fn the_four_markers_are_recognized() {
        assert_eq!(
            sniff(b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;0\x07"),
            vec![
                Signal::Marker(Osc133Marker::PromptStart),
                Signal::Marker(Osc133Marker::CommandStart),
                Signal::Marker(Osc133Marker::OutputStart),
                Signal::Marker(Osc133Marker::CommandEnd(Some(ExitCode(0)))),
            ]
        );
    }

    #[test]
    fn a_command_end_without_a_usable_code_still_ends_the_command() {
        assert_eq!(
            sniff(b"\x1b]133;D\x07"),
            vec![Signal::Marker(Osc133Marker::CommandEnd(None))]
        );
        assert_eq!(
            sniff(b"\x1b]133;D;not-a-number\x07"),
            vec![Signal::Marker(Osc133Marker::CommandEnd(None))]
        );
    }

    #[test]
    fn extra_parameters_after_the_exit_code_are_ignored() {
        assert_eq!(
            sniff(b"\x1b]133;D;3;aid=17\x07"),
            vec![Signal::Marker(Osc133Marker::CommandEnd(Some(ExitCode(3))))]
        );
    }

    #[test]
    fn other_osc_numbers_and_unknown_letters_are_not_markers() {
        assert!(sniff(b"\x1b]7;file:///tmp\x07").is_empty());
        assert!(sniff(b"\x1b]133;Z\x07").is_empty());
    }

    #[test]
    fn only_mode_1049_counts_as_a_screen_swap() {
        assert_eq!(
            sniff(b"\x1b[?1049h\x1b[?25h\x1b[?1049l"),
            vec![
                Signal::ScreenChanged(Screen::Alternate),
                Signal::ScreenChanged(Screen::Normal),
            ]
        );
    }
}
