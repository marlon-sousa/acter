//! Dependency-wiring test: proves the patched `vte` fork is what this workspace
//! actually builds against, and that unrecognized OSC sequences reach an embedder.
//!
//! This asserts a property of the dependency, not of acter's own code, so it lives in
//! `tests/` rather than in a module: B3 owns the `TerminalEngine` adapter and its design,
//! and nothing here should pre-empt it. Stock vte 0.15 fails these assertions — the
//! catch-all arm of `Performer::osc_dispatch` logs at `debug!` and discards, so `seen`
//! stays empty. If the root `[patch.crates-io]` entry is dropped or stops resolving,
//! `unhandled_osc` no longer exists and this file stops compiling.

use alacritty_terminal::vte::ansi::{Handler, Processor, StdSyncHandler};

/// Records every OSC that vte does not recognize, as owned UTF-8-lossy strings.
#[derive(Default)]
struct OscRecorder {
    seen: Vec<Vec<String>>,
    titles: Vec<Option<String>>,
}

impl Handler for OscRecorder {
    fn set_title(&mut self, title: Option<String>) {
        self.titles.push(title);
    }

    fn unhandled_osc(&mut self, params: &[&[u8]]) {
        self.seen.push(
            params
                .iter()
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .collect(),
        );
    }
}

fn drive(bytes: &[u8]) -> OscRecorder {
    let mut parser = Processor::<StdSyncHandler>::new();
    let mut recorder = OscRecorder::default();
    parser.advance(&mut recorder, bytes);
    recorder
}

/// An OSC 133 shell-integration marker arrives with its parameters split, in stream order.
#[test]
fn osc_133_marker_reaches_unhandled_osc() {
    // OSC 133 ; D ; 2 BEL — "command finished, exit status 2".
    let recorder = drive(b"\x1b]133;D;2\x07");

    assert_eq!(recorder.seen, vec![vec!["133", "D", "2"]]);
}

/// The escape hatch is for *unrecognized* sequences only: an OSC vte understands still
/// reaches its semantic `Handler` method and must not also surface as unhandled.
#[test]
fn recognized_osc_does_not_reach_unhandled_osc() {
    let recorder = drive(b"\x1b]0;window title\x07");

    assert_eq!(recorder.titles, vec![Some("window title".to_owned())]);
    assert!(recorder.seen.is_empty());
}

/// Markers interleaved with printed text keep their stream position, which is what lets a
/// downstream tracker cut command blocks over extracted text.
#[test]
fn markers_interleave_with_printed_text_in_stream_order() {
    let recorder =
        drive(b"\x1b]133;A\x07prompt$ \x1b]133;B\x07echo hi\x1b]133;C\x07hi\r\n\x1b]133;D;0\x07");

    assert_eq!(
        recorder.seen,
        vec![
            vec!["133", "A"],
            vec!["133", "B"],
            vec!["133", "C"],
            vec!["133", "D", "0"],
        ]
    );
}
