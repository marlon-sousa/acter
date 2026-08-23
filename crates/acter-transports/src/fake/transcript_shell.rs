//! Adapter: [`TranscriptShell`] — a [`FakeShell`] that answers from a
//! [`SessionTranscript`].
//!
//! **A request/response loop, not a linear tape** (spec B3.5, decision 4). It draws the
//! transcript's prompt, echoes what was submitted the way a terminal echoes what was
//! typed, plays the matching rule, and the pipe asks it for the prompt again. That is
//! what B2's regions actually require — `Prompt` is A..B, `CommandLine` is the B..C
//! echo, `Output` is C..D — and an event-level fake can produce neither a prompt nor an
//! echo, which is why DESIGN's echo exclusion had never been exercised against something
//! that echoes.
//!
//! **Everything here was in `ScriptedTransport` before B3.6 and behaves identically.**
//! What moved is the prompt sequence, the echo, the line discipline, rule matching and
//! the interrupt predicate; what stayed behind is the clock, the task and the channels.
//! Nothing in this file waits, and nothing in it knows where a read ends.

use std::mem::take;

use crate::scripted::transcript::{SessionTranscript, Step};

use super::shell::{Delivery, FakeShell, Script, Submission};

/// The line ending the echo appends, which is what a terminal shows when Enter is
/// pressed: the carriage return moves to column one, the line feed moves down.
const CRLF: &[u8] = b"\r\n";

/// What a console line editor treats as "discard whatever is pending on this line".
///
/// Line discipline, and therefore this shell's rather than the pipe's — the same side of
/// the seam as the echo and the prompt. It is modelled because B4.5 writes this byte at a
/// real `cmd.exe` and needs a fake that answers the way the real one was measured to: the
/// pending line is thrown away, and **nothing is echoed for it**. Echoing it instead is
/// what a raw byte pipe does, and it made a submitted `dir` come back as `ir` — the
/// emulator taking the escape and the letter after it for one sequence.
///
/// **Only a bare one**, and that distinction is measured rather than chosen. A console
/// turns input bytes into key events: an escape on its own is the escape *key* and clears
/// the line, while an escape followed by `[` is the start of a sequence and is echoed as
/// the literal characters it is made of — which is exactly why an unread cursor-position
/// answer sits in the buffer as `^[[3;1R` rather than quietly cancelling anything.
const CANCEL: u8 = 0x1b;

/// What follows an escape that makes it a sequence rather than the escape key.
const SEQUENCE: u8 = b'[';

/// A far end that says whatever its transcript says.
pub struct TranscriptShell {
    transcript: SessionTranscript,
}

impl TranscriptShell {
    pub fn new(transcript: SessionTranscript) -> Self {
        Self { transcript }
    }

    /// The built-in transcript: the ten scenarios A3 scripted as events, expressed as
    /// bytes.
    pub fn builtin() -> Self {
        Self::new(SessionTranscript::builtin())
    }

    /// The deliveries a run of steps becomes, each payload expanded to the exact bytes
    /// it puts on the wire.
    ///
    /// A payload that cannot be expanded ends the script there. Validation resolved
    /// every payload when the transcript was loaded, so reaching this means a capture
    /// file was removed underneath a running session; saying less is the only answer a
    /// synchronous shell has, because ending the session is the pipe's to do and this
    /// trait deliberately cannot reach it (decision 2, and the amendment recorded in the
    /// B3.6 spec).
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

    /// Bytes accumulate until a line is complete, and each complete line is one
    /// submission.
    ///
    /// Two exceptions. Bytes that exactly match a rule marked `interrupts` are submitted
    /// without waiting for a line ending, because a control byte never carries one — B3.5
    /// decision 7's whole point. And an escape discards everything pending on the line
    /// ahead of it, which is what a console line editor does with one and what B4.5 relies
    /// on. Everything else — a device-query answer, a partial line — simply accumulates.
    fn accept(&mut self, pending: &mut Vec<u8>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        loop {
            // Ahead of the line-ending scan, so a cancel arriving in the same read as the
            // line it precedes clears what was there rather than the line itself.
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
                // A carriage return and line feed together end one line, not two.
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

    /// A prompt, one rule, one interrupting rule and a default, so a test can say only
    /// the thing it is about.
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

    /// What a script would put on the wire, one string per delivery. Read boundaries are
    /// the pipe's, so this is deliberately not "the reads".
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

    /// A submission with no line ending is a control byte, and it is echoed as itself:
    /// appending a newline would move the cursor down a row the shell never left.
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

    /// The timing is the far end's: a command that dribbles output for a tenth of a
    /// second is the program being slow, and the script carries that unchanged.
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

    /// A control byte carries no line ending, so waiting for one would mean an interrupt
    /// that never lands.
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

    /// A device-query answer is written back mid-line and carries no line ending, so it
    /// must not be mistaken for a submitted command.
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

    /// The byte B4.5 writes ahead of a submitted line, and what a console line editor does
    /// with it: the pending line is thrown away and nothing is echoed for it.
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
