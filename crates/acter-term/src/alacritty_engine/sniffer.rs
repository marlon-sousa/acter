//! Adapter (internal to the terminal-engine adapter): the stream-position sniffer.
//!
//! Its entire job is *where* in the byte stream something happened. It models no
//! terminal state at all — the emulator does that — so of `ansi::Handler`'s seventy-two
//! methods it implements exactly three and leaves the rest as the trait's default
//! no-ops. Here that is correct rather than merely convenient: a method this type does
//! not implement is a method it has no business implementing.
//!
//! Nothing is forwarded to the emulator from here, which is the point. Every `Handler`
//! method has a default body, so a wrapper that forwarded all seventy-two would keep
//! compiling when a future vte release *adds* one, silently pick up the new default
//! no-op, and stop forwarding that capability — vte did exactly that in 0.13.0
//! (`set_private_mode`, `unset_private_mode`, `report_mode`, `report_private_mode`) and
//! again in 0.13.1 (SCP). Nothing forwards here, so nothing can be forgotten
//! (spec B3, decision 1).

use std::str::from_utf8;

use acter_core::{ExitCode, Osc133Marker, Screen};
use alacritty_terminal::vte::ansi::{Handler, NamedPrivateMode, PrivateMode};

/// Something the sniffer noticed at the current point in the byte stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Signal {
    /// A recognized OSC 133 shell-integration marker.
    Marker(Osc133Marker),
    /// The emulator is about to swap screens.
    ScreenChanged(Screen),
}

/// Collects signals for the byte currently being parsed. The engine drains it after
/// every byte, so the queue never holds more than one sequence's worth.
#[derive(Debug, Default)]
pub(super) struct Sniffer {
    signals: Vec<Signal>,
}

impl Sniffer {
    /// Whether the byte just parsed completed something the engine must place.
    pub(super) fn signalled(&self) -> bool {
        !self.signals.is_empty()
    }

    /// Drains what the byte just parsed produced, in order.
    pub(super) fn drain(&mut self) -> Vec<Signal> {
        self.signals.drain(..).collect()
    }
}

impl Handler for Sniffer {
    /// The fork's escape hatch. vte parses OSC sequences it does not recognize and then
    /// discards them, which would make shell-integration markers unobservable; the
    /// patched crate hands them here instead. It carries no OSC 133 knowledge, so
    /// interpreting the parameters is this crate's job.
    fn unhandled_osc(&mut self, params: &[&[u8]]) {
        if let Some(marker) = parse_osc133(params) {
            self.signals.push(Signal::Marker(marker));
        }
    }

    /// The alternate screen arrives as private mode 1049, not as a `Handler` method of
    /// its own — so this pair is how the switch becomes locatable in the stream.
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

/// Exactly the mode the emulator itself keys `ALT_SCREEN` on, so the sniffer and the
/// emulator can never disagree about which screen is current.
fn swaps_screen(mode: PrivateMode) -> bool {
    matches!(
        mode,
        PrivateMode::Named(NamedPrivateMode::SwapScreenAndSetRestoreCursor)
    )
}

/// Reads an OSC 133 marker out of a sequence's parameters, or `None` if this is some
/// other OSC entirely — which is the ordinary case, and the reason the fork's hook is
/// generic rather than marker-aware.
fn parse_osc133(params: &[&[u8]]) -> Option<Osc133Marker> {
    if params.first().copied() != Some(b"133".as_slice()) {
        return None;
    }

    match params.get(1).copied()? {
        b"A" => Some(Osc133Marker::PromptStart),
        b"B" => Some(Osc133Marker::CommandStart),
        b"C" => Some(Osc133Marker::OutputStart),
        // Any parameter after the exit code is a `key=value` extra some shells append
        // (`aid=`, and similar) and is ignored. A missing or unparseable code becomes a
        // marker with no code rather than no marker: the block still ended, and B2's
        // `Option<ExitCode>` exists for exactly this.
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
