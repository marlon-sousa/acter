//! Adapter: [`Unmarked`] — a [`FakeShell`] decorator that emits no shell-integration
//! markers.

use super::shell::{FakeShell, Script, Submission};

const OSC133: &[u8] = b"\x1b]133;";
const BEL: u8 = 0x07;
const ST: &[u8] = b"\x1b\\";

pub struct Unmarked<S> {
    shell: S,
}

impl<S: FakeShell> Unmarked<S> {
    pub fn new(shell: S) -> Self {
        Self { shell }
    }

    fn unmarked(mut script: Script) -> Script {
        script.rewrite(without_markers);
        script
    }
}

impl<S: FakeShell> FakeShell for Unmarked<S> {
    fn greet(&mut self) -> Script {
        Self::unmarked(self.shell.greet())
    }

    fn accept(&mut self, pending: &mut Vec<u8>) -> Vec<Submission> {
        self.shell.accept(pending)
    }

    fn interrupts(&self, submission: &Submission) -> bool {
        self.shell.interrupts(submission)
    }

    fn answer(&mut self, submission: &Submission) -> Script {
        Self::unmarked(self.shell.answer(submission))
    }
}

/// A sequence with no terminator runs to the end of the delivery.
fn without_markers(bytes: &[u8]) -> Vec<u8> {
    let mut kept = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some(start) = position_of(rest, OSC133) {
        kept.extend_from_slice(&rest[..start]);
        let inside = &rest[start + OSC133.len()..];
        rest = match end_of_marker(inside) {
            Some(end) => &inside[end..],
            None => &[],
        };
    }
    kept.extend_from_slice(rest);
    kept
}

fn end_of_marker(inside: &[u8]) -> Option<usize> {
    let bel = inside.iter().position(|byte| *byte == BEL).map(|at| at + 1);
    let st = position_of(inside, ST).map(|at| at + ST.len());
    match (bel, st) {
        (Some(bel), Some(st)) => Some(bel.min(st)),
        (found, None) | (None, found) => found,
    }
}

fn position_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use crate::fake::TranscriptShell;

    use super::*;

    fn said(script: &Script) -> Vec<String> {
        script
            .deliveries()
            .iter()
            .map(|delivery| String::from_utf8_lossy(delivery.bytes()).into_owned())
            .collect()
    }

    fn line(text: &str) -> Submission {
        Submission::new(text.as_bytes().to_vec(), true)
    }

    fn builtin() -> Unmarked<TranscriptShell> {
        Unmarked::new(TranscriptShell::builtin())
    }

    #[test]
    fn the_prompt_is_still_drawn_but_carries_no_markers() {
        assert_eq!(
            said(&builtin().greet()),
            ["", "acter> ", ""],
            "the prompt text survives; the A and B markers are gone entirely"
        );
    }

    #[test]
    fn the_session_still_answers_every_rule_it_knows() {
        let mut shell = builtin();

        assert_eq!(
            said(&shell.answer(&line("small"))),
            ["small\r\n", "", "hello from acter\r\n", ""],
            "the echo and the output are untouched; only the C and D markers went"
        );
        assert_eq!(
            said(&shell.answer(&line("fail")))[2],
            "error: the command reported a problem\r\n",
            "an exit code the marker carried is not text, so nothing of it is left"
        );
    }

    #[test]
    fn no_osc_133_sequence_reaches_the_pipe_from_anything_it_says() {
        let mut shell = builtin();
        let mut scripts = vec![shell.greet()];
        for scenario in [
            "small",
            "big",
            "fail",
            "slow",
            "forever",
            "nano",
            "tail",
            "burst",
            "speech",
            "nothing scripted this",
        ] {
            scripts.push(shell.answer(&line(scenario)));
        }

        for said in scripts.iter().flat_map(said) {
            assert!(
                !said.contains("\x1b]133;"),
                "a marker survived the decorator: {said:?}"
            );
        }
    }

    #[test]
    fn text_around_a_marker_survives_untouched() {
        assert_eq!(
            without_markers(b"before\x1b]133;D;0\x07after"),
            b"beforeafter"
        );
        assert_eq!(
            without_markers(b"\x1b[?1049hpainting"),
            b"\x1b[?1049hpainting",
            "an alt-screen switch is not shell integration"
        );
        assert_eq!(without_markers(b"plain"), b"plain");
    }

    #[test]
    fn a_marker_terminated_with_st_is_removed_too() {
        assert_eq!(without_markers(b"a\x1b]133;A\x1b\\b"), b"ab");
    }

    #[test]
    fn an_unterminated_marker_takes_the_rest_of_the_delivery_with_it() {
        assert_eq!(without_markers(b"kept\x1b]133;D;0"), b"kept");
    }

    #[test]
    fn line_discipline_and_interrupts_pass_straight_through() {
        let mut shell = builtin();
        let mut pending = b"small\nhal".to_vec();

        assert_eq!(shell.accept(&mut pending), [line("small")]);
        assert_eq!(pending, b"hal");
        assert!(shell.interrupts(&Submission::new(vec![0x03], false)));
        assert!(!shell.interrupts(&line("small")));
    }
}
