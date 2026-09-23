//! Adapter: [`TranscriptShell`] — a [`FakeShell`] that answers from a
//! [`SessionTranscript`].

use std::mem::take;

use crate::scripted::transcript::{SessionTranscript, Step};

use super::shell::{Delivery, FakeShell, Script, Submission};

const CRLF: &[u8] = b"\r\n";

/// Only a bare escape discards the pending line; `cmd.exe` echoes an escape followed by `[`
/// as literal characters (see docs/specs/b4.5-cmd-markers-and-unclaimed-replies.md).
const CANCEL: u8 = 0x1b;

const SEQUENCE: u8 = b'[';

pub struct TranscriptShell {
    transcript: SessionTranscript,
}

impl TranscriptShell {
    pub fn new(transcript: SessionTranscript) -> Self {
        Self { transcript }
    }

    pub fn builtin() -> Self {
        Self::new(SessionTranscript::builtin())
    }

    /// A payload that cannot be expanded, such as a capture file removed under a running
    /// session, ends the script there.
    fn deliveries(&self, steps: &[Step]) -> Vec<Delivery> {
        let mut deliveries = Vec::with_capacity(steps.len());
        for step in steps {
            let Ok(bytes) = self.transcript.expand(step.payload()) else {
                break;
            };
            deliveries.push(Delivery::new(step.delay(), bytes, step.repeat()));
        }
        deliveries
    }
}

impl FakeShell for TranscriptShell {
    fn greet(&mut self) -> Script {
        Script::new(self.deliveries(self.transcript.prompt()))
    }

    fn accept(&mut self, pending: &mut Vec<u8>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        loop {
            // Ahead of the line-ending scan, so a cancel in the same read as a line clears
            // only what preceded it.
            if let Some(index) = pending
                .iter()
                .position(|byte| *byte == CANCEL)
                .filter(|at| pending.get(at + 1) != Some(&SEQUENCE))
            {
                pending.drain(..index + 1);
                continue;
            }
            if let Some(index) = pending
                .iter()
                .position(|byte| *byte == b'\r' || *byte == b'\n')
            {
                let line = pending[..index].to_vec();
                let pair =
                    usize::from(pending[index] == b'\r' && pending.get(index + 1) == Some(&b'\n'));
                pending.drain(..index + 1 + pair);
                submissions.push(Submission::new(line, true));
                continue;
            }
            if !pending.is_empty()
                && self
                    .transcript
                    .interrupts(&String::from_utf8_lossy(pending))
            {
                submissions.push(Submission::new(take(pending), false));
                continue;
            }
            return submissions;
        }
    }

    fn interrupts(&self, submission: &Submission) -> bool {
        self.transcript.interrupts(&submission.line())
    }

    fn answer(&mut self, submission: &Submission) -> Script {
        let mut echo = submission.bytes().to_vec();
        if submission.terminated() {
            echo.extend_from_slice(CRLF);
        }

        let mut deliveries = Vec::new();
        if !echo.is_empty() {
            deliveries.push(Delivery::instant(echo));
        }
        deliveries.extend(self.deliveries(self.transcript.rule_for(&submission.line()).steps()));
        Script::new(deliveries)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn shell() -> TranscriptShell {
        TranscriptShell::new(
            SessionTranscript::parse(
                r#"{
                  "on_start": [
                    { "payload": { "marker": { "kind": "A" } } },
                    { "payload": { "text": "> " } },
                    { "payload": { "marker": { "kind": "B" } } }
                  ],
                  "rules": [
                    {
                      "match": "go",
                      "steps": [
                        { "payload": { "marker": { "kind": "C" } } },
                        {
                          "delay": { "min_ms": 100, "max_ms": 100 },
                          "payload": { "text": "going\r\n" }
                        }
                      ]
                    },
                    {
                      "match": "\u0003",
                      "interrupts": true,
                      "steps": [{ "payload": { "text": "^C\r\n" } }]
                    }
                  ],
                  "default": { "steps": [{ "payload": { "text": "?\r\n" } }] }
                }"#,
            )
            .expect("the test transcript parses"),
        )
    }

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

    #[test]
    fn greeting_draws_the_prompt_sequence() {
        assert_eq!(
            said(&shell().greet()),
            ["\x1b]133;A\x07", "> ", "\x1b]133;B\x07"],
            "prompt start, the prompt itself, then command-line start"
        );
    }

    #[test]
    fn an_answer_echoes_first_and_then_plays_the_rule() {
        assert_eq!(
            said(&shell().answer(&line("go"))),
            ["go\r\n", "\x1b]133;C\x07", "going\r\n"],
            "the terminal echoes what was typed, then the command runs"
        );
    }

    #[test]
    fn an_unterminated_submission_is_echoed_without_a_line_ending() {
        let mut shell = shell();
        let interrupt = Submission::new(vec![0x03], false);

        assert_eq!(said(&shell.answer(&interrupt))[0], "\u{3}");
    }

    #[test]
    fn an_unrecognized_line_takes_the_default_rule() {
        assert_eq!(
            said(&shell().answer(&line("nothing scripted this"))),
            ["nothing scripted this\r\n", "?\r\n"]
        );
    }

    #[test]
    fn a_delivery_carries_the_transcripts_own_delay() {
        let script = shell().answer(&line("go"));
        let delays: Vec<_> = script
            .deliveries()
            .iter()
            .map(|delivery| delivery.delay().pick(0))
            .collect();

        assert_eq!(
            delays,
            [Duration::ZERO, Duration::ZERO, Duration::from_millis(100)],
            "the echo and the C marker are instant; the output waits"
        );
    }

    #[test]
    fn a_complete_line_is_taken_and_a_partial_one_is_left() {
        let mut shell = shell();
        let mut pending = b"first\nsec".to_vec();

        let submissions = shell.accept(&mut pending);

        assert_eq!(submissions, [line("first")]);
        assert_eq!(
            pending, b"sec",
            "the remainder waits for the rest of its line"
        );
        assert!(
            shell.accept(&mut pending).is_empty(),
            "an unterminated write is not a command"
        );
    }

    #[test]
    fn a_carriage_return_and_line_feed_end_one_line_and_not_two() {
        let mut shell = shell();
        let mut pending = b"first\r\nsecond\r\n".to_vec();

        assert_eq!(
            shell.accept(&mut pending),
            [line("first"), line("second")],
            "two lines, not four"
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn a_control_byte_is_accepted_with_no_line_ending() {
        let mut shell = shell();
        let mut pending = vec![0x03];

        let submissions = shell.accept(&mut pending);

        assert_eq!(submissions, [Submission::new(vec![0x03], false)]);
        assert!(pending.is_empty());
        assert!(shell.interrupts(&submissions[0]));
        assert!(
            !shell.interrupts(&line("go")),
            "an ordinary line waits its turn"
        );
    }

    #[test]
    fn a_device_query_answer_is_not_a_submission() {
        let mut shell = shell();
        let mut pending = b"\x1b[1;1R".to_vec();

        assert!(shell.accept(&mut pending).is_empty());
        assert_eq!(
            pending, b"\x1b[1;1R",
            "it stays pending, and it stays intact"
        );
    }

    #[test]
    fn a_bare_escape_discards_the_pending_line() {
        let mut shell = shell();
        let mut pending = b"half a line\x1bgo\r".to_vec();

        let submissions = shell.accept(&mut pending);

        assert_eq!(submissions.len(), 1);
        assert_eq!(
            submissions[0].bytes(),
            b"go",
            "what was pending ahead of the escape is gone, and the escape with it"
        );
    }

    #[test]
    fn the_builtin_shell_answers_every_scenario() {
        let mut shell = TranscriptShell::builtin();
        let unknown = said(&shell.answer(&line("something nobody ever scripted")));

        for scenario in [
            "small", "big", "fail", "slow", "forever", "nano", "tail", "burst", "speech",
        ] {
            assert_ne!(
                said(&shell.answer(&line(scenario))),
                unknown,
                "the built-in shell must answer {scenario}"
            );
        }
    }
}
