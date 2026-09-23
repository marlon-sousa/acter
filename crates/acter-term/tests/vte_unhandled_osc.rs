//! Dependency-wiring test: the patched `vte` fork is what this workspace builds against, and
//! unrecognized OSC sequences reach an embedder.
//!
//! Stock vte 0.15 fails these assertions: `Performer::osc_dispatch` logs an unrecognized OSC
//! at `debug!` and discards it.

use alacritty_terminal::vte::ansi::{Handler, Processor, StdSyncHandler};

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

#[test]
fn osc_133_marker_reaches_unhandled_osc() {
    let recorder = drive(b"\x1b]133;D;2\x07");

    assert_eq!(recorder.seen, vec![vec!["133", "D", "2"]]);
}

#[test]
fn recognized_osc_does_not_reach_unhandled_osc() {
    let recorder = drive(b"\x1b]0;window title\x07");

    assert_eq!(recorder.titles, vec![Some("window title".to_owned())]);
    assert!(recorder.seen.is_empty());
}

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
