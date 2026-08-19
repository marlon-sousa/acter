//! Entity/value: the session transcript — the JSON a scripted session is played from,
//! its validation, and the expansion of a step's payload into the exact bytes that go
//! on the wire.
//!
//! **One format, two consumers.** A cargo test parses a transcript and feeds its bytes
//! straight to the terminal engine — no transport, no runtime, no clock — which is
//! ARCHITECTURE's golden-transcript tier. [`ScriptedTransport`](super::ScriptedTransport)
//! replays the same file as a live session. There is deliberately no second dialect for
//! tests (spec B3.5, decision 5).
//!
//! **A step is a read boundary.** Each step is one delivery to whoever is reading, so
//! chunking is scriptable: a marker split across two reads is two steps with zero delay,
//! which is DESIGN's reliability case and the thing B3's own test had to hand-write.
//!
//! **The marker shorthand is authoring convenience, never a bypass.** A `marker` payload
//! expands to the real OSC 133 sequence terminated with BEL, so the recognizer in
//! `acter-term` sees exactly the bytes a PowerShell prompt hook would emit. A fixture
//! that wants the ST terminator, a malformed marker, or a forged one writes those bytes
//! itself, with `text` or `base64` (decision 6).
//!
//! Pure data: no clock, no randomness, and no I/O beyond reading a `file` payload
//! relative to the transcript it was named in.

use std::fs::{read, read_to_string};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

/// The escape sequence a `marker` payload expands into, up to the marker letter.
const OSC133: &[u8] = b"\x1b]133;";
/// BEL, the terminator the shorthand uses. Real shells emit this one and ST; a fixture
/// that wants ST writes the bytes itself.
const BEL: u8 = 0x07;

/// One scripted session: how it greets, how it answers each line it knows, and what it
/// does with a line it does not.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTranscript {
    /// The prompt sequence. Emitted when the session starts and again after every rule
    /// that runs to completion — that is what makes this a shell rather than a tape
    /// (decision 4). Conventionally prompt-start, the prompt text, command-line-start.
    #[serde(default)]
    on_start: Vec<Step>,
    /// The lines this session knows how to answer, matched in order.
    rules: Vec<Rule>,
    /// What any other line gets: A3 decision 4's echo and return to the prompt.
    default: Rule,
    /// Where a `file` payload resolves from: the directory the transcript was read from.
    /// `None` for a transcript that came from a string, which is why a `file` payload is
    /// then rejected rather than guessed at.
    #[serde(skip)]
    base: Option<PathBuf>,
}

/// One line this session answers, and how.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    /// The submitted line this rule answers, matched exactly. `None` on the default
    /// rule, which is reached by not matching anything else.
    #[serde(rename = "match", default)]
    line: Option<String>,
    /// Whether this rule cancels whatever sequence is in flight and runs instead.
    ///
    /// That covers a written `0x03` exactly as a real shell would answer it, and it
    /// covers A3.1's literal `stop` line, without inventing stdin semantics a fake has
    /// no business having. Which bytes the frontend sends on Ctrl+C is A3.2's question;
    /// this makes both answers testable rather than deciding either (decision 7).
    #[serde(default)]
    interrupts: bool,
    /// What the rule emits, in order.
    steps: Vec<Step>,
}

/// One delivery: a wait, then one payload, optionally repeated.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Step {
    /// How long to wait before this delivery. Absent means no wait at all, which is how
    /// a payload is split across two reads with nothing observable between them.
    #[serde(default)]
    delay: DelayRange,
    payload: Payload,
    /// How many times to deliver it. Absent means once.
    #[serde(default)]
    repeat: Repeat,
}

/// A delay as an inclusive millisecond range, reusing A3's shape: equal bounds are
/// deterministic, unequal bounds are sampled per delivery so a manual session paces
/// organically rather than metronomically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DelayRange {
    min_ms: u64,
    max_ms: u64,
}

/// How many times a step delivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub(crate) enum Repeat {
    /// Exactly this many deliveries: `"repeat": 12`.
    Times(u32),
    /// Never stops: `"repeat": "forever"` — what `forever` and `tail` need, and the only
    /// way a transcript can decline to ever end a command.
    Endless(Endless),
}

/// The one endless spelling, as its own type so `"repeat": "whenever"` is rejected
/// rather than quietly read as something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Endless {
    Forever,
}

/// What one delivery carries.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Payload {
    /// A UTF-8 string, so JSON's own escapes reach the byte stream: `"\r\n"` is a real
    /// carriage return and line feed, and a JSON unicode escape carries a
    /// real escape byte.
    Text(String),
    /// An OSC 133 marker by letter, expanded to the real sequence.
    Marker {
        kind: MarkerKind,
        /// Only a `D` carries one.
        #[serde(default)]
        exit_code: Option<i32>,
    },
    /// Arbitrary bytes, standard base64 with padding — for a capture too small to be
    /// worth its own file and too binary to write as text.
    Base64(String),
    /// A raw capture, named relative to the transcript file. What B5's recorded golden
    /// transcripts arrive as.
    File(String),
}

/// The four markers, spelled as the shell emits them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum MarkerKind {
    #[serde(rename = "A")]
    PromptStart,
    #[serde(rename = "B")]
    CommandStart,
    #[serde(rename = "C")]
    OutputStart,
    #[serde(rename = "D")]
    CommandEnd,
}

impl MarkerKind {
    fn letter(self) -> u8 {
        match self {
            Self::PromptStart => b'A',
            Self::CommandStart => b'B',
            Self::OutputStart => b'C',
            Self::CommandEnd => b'D',
        }
    }
}

impl Default for DelayRange {
    /// No wait: the common case for the steps that exist only to cut a read.
    fn default() -> Self {
        Self::fixed(0)
    }
}

impl DelayRange {
    pub(crate) const fn fixed(ms: u64) -> Self {
        Self {
            min_ms: ms,
            max_ms: ms,
        }
    }

    /// Whether this range asks for no wait at all, in which case no timer is requested
    /// from the clock — a zero wait is not a wait, and asking for one would make every
    /// read-boundary step depend on a clock tick.
    pub(crate) const fn is_instant(self) -> bool {
        self.max_ms == 0
    }

    /// The delay for one delivery. `roll` is the caller's source of variation, used only
    /// when the bounds differ — so equal bounds never consume it and a fixed transcript
    /// is reproducible to the millisecond.
    pub(crate) const fn pick(self, roll: u64) -> Duration {
        let ms = if self.max_ms <= self.min_ms {
            self.min_ms
        } else {
            self.min_ms + roll % (self.max_ms - self.min_ms + 1)
        };
        Duration::from_millis(ms)
    }
}

impl Default for Repeat {
    fn default() -> Self {
        Self::Times(1)
    }
}

impl Step {
    pub(crate) fn delay(&self) -> DelayRange {
        self.delay
    }

    pub(crate) fn payload(&self) -> &Payload {
        &self.payload
    }

    pub(crate) fn repeat(&self) -> Repeat {
        self.repeat
    }
}

impl Rule {
    pub(crate) fn steps(&self) -> &[Step] {
        &self.steps
    }
}

impl SessionTranscript {
    /// The built-in transcript: the ten scenarios A3 scripted as events, expressed as
    /// bytes. Embedded, so a dev run needs no file at all — A3's built-in defaults,
    /// moved down a layer.
    ///
    /// Panics if it does not parse, which is a build-time fact rather than a runtime
    /// condition: the file is compiled in, and a test below holds it to that.
    pub fn builtin() -> Self {
        Self::parse(include_str!("default_transcript.json"))
            .expect("the built-in transcript must parse")
    }

    /// Parses and validates a transcript from JSON. A `file` payload is rejected,
    /// because a string has no directory to resolve one against.
    ///
    /// The error is a speakable sentence: an unusable transcript is a loud failure, not
    /// a silent fallback (A3's precedent for its script config).
    pub fn parse(json: &str) -> Result<Self, String> {
        let transcript: Self = serde_json::from_str(json)
            .map_err(|e| format!("The session transcript is not valid JSON. {e}"))?;
        transcript.validate()?;
        Ok(transcript)
    }

    /// Reads and validates a transcript from disk. `file` payloads resolve relative to
    /// the transcript's own directory, so a transcript and its captures move together.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let json = read_to_string(path).map_err(|e| {
            format!(
                "The session transcript at {} could not be read. {e}",
                path.display()
            )
        })?;
        let mut transcript: Self = serde_json::from_str(&json).map_err(|e| {
            format!(
                "The session transcript at {} is not valid JSON. {e}",
                path.display()
            )
        })?;
        transcript.base = Some(path.parent().unwrap_or(Path::new(".")).to_path_buf());
        transcript.validate()?;
        Ok(transcript)
    }

    /// The prompt sequence: emitted at the start of the session and again after each
    /// completed rule.
    pub(crate) fn prompt(&self) -> &[Step] {
        &self.on_start
    }

    /// The rule that answers `line`, falling back to the default rule. Matching is exact
    /// on the whole submitted line, which is A3 decision 4 unchanged.
    pub(crate) fn rule_for(&self, line: &str) -> &Rule {
        self.rules
            .iter()
            .find(|rule| rule.line.as_deref() == Some(line))
            .unwrap_or(&self.default)
    }

    /// Whether `line` is answered by a rule that cancels what is in flight. Asked before
    /// a line is complete, so a written `0x03` can be acted on without waiting for a
    /// newline that a control byte never carries.
    pub(crate) fn interrupts(&self, line: &str) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.line.as_deref() == Some(line) && rule.interrupts)
    }

    /// The exact bytes one payload puts on the wire.
    pub(crate) fn expand(&self, payload: &Payload) -> Result<Vec<u8>, String> {
        match payload {
            Payload::Text(text) => Ok(text.as_bytes().to_vec()),
            Payload::Marker { kind, exit_code } => {
                let mut bytes = OSC133.to_vec();
                bytes.push(kind.letter());
                if let Some(code) = exit_code {
                    bytes.push(b';');
                    bytes.extend_from_slice(code.to_string().as_bytes());
                }
                bytes.push(BEL);
                Ok(bytes)
            }
            Payload::Base64(encoded) => decode_base64(encoded),
            Payload::File(name) => {
                let base = self.base.as_deref().ok_or_else(|| {
                    format!(
                        "The transcript names the capture file {name}, but it was not loaded from \
                         disk, so there is no folder to look in."
                    )
                })?;
                let path = base.join(name);
                read(&path).map_err(|e| {
                    format!(
                        "The transcript names the capture file {}, which could not be read. {e}",
                        path.display()
                    )
                })
            }
        }
    }

    /// Everything a transcript must satisfy before anything replays it. Failing here is
    /// the point: an authoring mistake that survives into a session shows up as a
    /// terminal that behaves oddly, which is the hardest kind of defect to hear.
    fn validate(&self) -> Result<(), String> {
        if self.rules.is_empty() {
            return Err(
                "The transcript has no rules, so it could not answer any command.".to_owned(),
            );
        }
        if self.default.line.is_some() {
            return Err(
                "The transcript's default rule has a match, but the default rule is the \
                        one reached by matching nothing."
                    .to_owned(),
            );
        }
        if self.default.interrupts {
            return Err(
                "The transcript's default rule interrupts, but the rule for an ordinary \
                        line must not cancel what is running."
                    .to_owned(),
            );
        }

        self.validate_steps("the prompt", &self.on_start)?;
        for rule in &self.rules {
            let line = rule.line.as_deref().ok_or_else(|| {
                "The transcript has a rule with no match, so nothing could ever select it."
                    .to_owned()
            })?;
            self.validate_steps(&format!("the rule for {line}"), &rule.steps)?;
        }
        self.validate_steps("the default rule", &self.default.steps)
    }

    fn validate_steps(&self, whose: &str, steps: &[Step]) -> Result<(), String> {
        for (index, step) in steps.iter().enumerate() {
            let step_number = index + 1;
            if step.delay.min_ms > step.delay.max_ms {
                return Err(format!(
                    "Step {step_number} of {whose} waits between {} and {} milliseconds, which is \
                     backwards.",
                    step.delay.min_ms, step.delay.max_ms
                ));
            }
            match step.repeat {
                Repeat::Times(0) => {
                    return Err(format!(
                        "Step {step_number} of {whose} repeats zero times, so it would never \
                         happen."
                    ));
                }
                Repeat::Endless(_) if step.delay.is_instant() => {
                    return Err(format!(
                        "Step {step_number} of {whose} repeats forever with no delay, so it would \
                         never pause to let anything else happen."
                    ));
                }
                _ => {}
            }
            if matches!(
                step.payload,
                Payload::Marker {
                    kind,
                    exit_code: Some(_),
                } if kind != MarkerKind::CommandEnd
            ) {
                return Err(format!(
                    "Step {step_number} of {whose} puts an exit code on a marker that is not a \
                     command end."
                ));
            }
            self.expand(&step.payload)
                .map_err(|why| format!("Step {step_number} of {whose} could not be read. {why}"))?;
        }
        Ok(())
    }
}

/// Decodes standard base64 with padding. Hand-written rather than taken as a dependency:
/// this crate's dependency list is one of the spec's acceptance criteria, and a decoder
/// for a fixture format is thirty lines.
fn decode_base64(encoded: &str) -> Result<Vec<u8>, String> {
    let mut bits: u32 = 0;
    let mut held = 0;
    let mut bytes = Vec::new();
    for symbol in encoded.chars().filter(|symbol| !symbol.is_whitespace()) {
        if symbol == '=' {
            break;
        }
        let value = base64_value(symbol).ok_or_else(|| {
            format!("A base64 payload contains {symbol}, which is not a base64 character.")
        })?;
        bits = (bits << 6) | u32::from(value);
        held += 6;
        if held >= 8 {
            held -= 8;
            bytes.push((bits >> held) as u8);
        }
    }
    Ok(bytes)
}

fn base64_value(symbol: char) -> Option<u8> {
    let value = match symbol {
        'A'..='Z' => symbol as u8 - b'A',
        'a'..='z' => symbol as u8 - b'a' + 26,
        '0'..='9' => symbol as u8 - b'0' + 52,
        '+' => 62,
        '/' => 63,
        _ => return None,
    };
    Some(value)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use acter_core::PacingConfig;

    use super::*;

    /// The reliability transcripts, which are fixtures for the pipeline test and load
    /// cases for this one.
    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name)
    }

    fn steps_of<'a>(transcript: &'a SessionTranscript, line: &str) -> &'a [Step] {
        transcript.rule_for(line).steps()
    }

    fn text_of(step: &Step) -> &str {
        match step.payload() {
            Payload::Text(text) => text,
            other => panic!("expected a text payload, got {other:?}"),
        }
    }

    /// A transcript with one rule and one default, so a test can say only the thing it
    /// is about.
    fn one_rule(rule_json: &str) -> Result<SessionTranscript, String> {
        SessionTranscript::parse(&format!(
            r#"{{ "rules": [{rule_json}], "default": {{ "steps": [] }} }}"#
        ))
    }

    #[test]
    fn the_builtin_transcript_parses_and_answers_every_scenario() {
        let transcript = SessionTranscript::builtin();
        let default = transcript.rule_for("something nobody ever scripted");

        for scenario in [
            "small", "big", "fail", "slow", "forever", "nano", "tail", "burst", "speech", "stop",
        ] {
            assert_ne!(
                transcript.rule_for(scenario),
                default,
                "the built-in transcript must answer {scenario}"
            );
        }
    }

    /// Decision 9's whole point: A3's fake scripted every verdict and this one scripts
    /// none, so the numbers have to earn their scenarios against the real thresholds.
    /// If `PacingConfig`'s defaults ever move, this test says which scenario stopped
    /// meaning what it was written to mean.
    #[test]
    fn the_builtin_numbers_are_chosen_against_the_pacing_defaults() {
        let transcript = SessionTranscript::builtin();
        let config = PacingConfig::default();
        let quiescence = config.quiescence.as_millis() as u64;

        let big = text_of(&steps_of(&transcript, "big")[1]);
        assert!(
            big.lines().count() > config.max_lines,
            "big must cross the line threshold on its own, not by being called too big"
        );

        let burst = text_of(&steps_of(&transcript, "burst")[1]);
        assert!(
            burst.lines().count() > config.max_lines,
            "burst's flood must cross the threshold in one step, before any gap"
        );

        let speech = text_of(&steps_of(&transcript, "speech")[1]);
        assert!(
            speech.lines().count() <= config.max_lines
                && speech.chars().count() <= config.max_chars,
            "speech must stay under both caps, or it is read as a size instead of aloud"
        );

        for step in &steps_of(&transcript, "slow")[1..] {
            assert!(
                step.delay().min_ms > quiescence,
                "each of slow's phases must be followed by a real quiescent gap"
            );
        }

        let forever = steps_of(&transcript, "forever");
        let endless = forever.last().expect("forever has steps");
        assert!(
            matches!(endless.repeat(), Repeat::Endless(_)),
            "forever never ends"
        );
        assert!(
            endless.delay().max_ms < quiescence,
            "forever's stream must never leave a quiescent gap, or the patience window \
             restarts and nothing is ever announced"
        );
        assert!(
            !forever.iter().any(|step| matches!(
                step.payload(),
                Payload::Marker {
                    kind: MarkerKind::CommandEnd,
                    ..
                }
            )),
            "forever ends with no command end at all: DESIGN's first reliability case"
        );
    }

    /// The five the pipeline test does not replay end to end. They parse, they match,
    /// and they ask for the delays they claim to; how they sound is B6's manual matrix.
    #[test]
    fn the_scenarios_asserted_at_transcript_level_request_the_delays_they_claim() {
        let transcript = SessionTranscript::builtin();

        let tail = steps_of(&transcript, "tail");
        assert_eq!(tail[1].repeat(), Repeat::Times(10));
        assert!(
            tail[1].delay().min_ms < tail[1].delay().max_ms,
            "tail is the sampled-range scenario"
        );

        let burst = steps_of(&transcript, "burst");
        assert_eq!(
            burst[2].repeat(),
            Repeat::Times(4),
            "a flood, then a trickle"
        );

        assert_eq!(
            steps_of(&transcript, "slow").len(),
            5,
            "start, three phases, end"
        );
        assert_eq!(steps_of(&transcript, "speech").len(), 3);
        assert_eq!(steps_of(&transcript, "forever").len(), 4);
    }

    #[test]
    fn a_transcript_round_trips_from_json() {
        let transcript = SessionTranscript::parse(
            r#"{
              "on_start": [
                { "payload": { "marker": { "kind": "A" } } },
                { "payload": { "text": "> " } },
                { "payload": { "marker": { "kind": "B" } } }
              ],
              "rules": [
                {
                  "match": "hello",
                  "steps": [
                    { "payload": { "marker": { "kind": "C" } } },
                    {
                      "delay": { "min_ms": 10, "max_ms": 20 },
                      "payload": { "text": "hi\r\n" },
                      "repeat": 3
                    },
                    { "payload": { "marker": { "kind": "D", "exit_code": 1 } } }
                  ]
                },
                { "match": "quit", "interrupts": true, "steps": [] }
              ],
              "default": { "steps": [{ "payload": { "marker": { "kind": "D" } } }] }
            }"#,
        )
        .expect("a full transcript parses");

        assert_eq!(transcript.prompt().len(), 3);
        let hello = steps_of(&transcript, "hello");
        assert_eq!(
            hello[1].delay(),
            DelayRange {
                min_ms: 10,
                max_ms: 20
            }
        );
        assert_eq!(hello[1].repeat(), Repeat::Times(3));
        assert_eq!(text_of(&hello[1]), "hi\r\n");
        assert!(transcript.interrupts("quit"));
        assert!(!transcript.interrupts("hello"));
        assert_eq!(steps_of(&transcript, "anything else").len(), 1);
    }

    #[test]
    fn each_payload_kind_expands_to_the_exact_bytes() {
        let transcript = SessionTranscript::builtin();

        assert_eq!(
            transcript
                .expand(&Payload::Text("hi\r\n".to_owned()))
                .expect("text expands"),
            b"hi\r\n"
        );
        assert_eq!(
            transcript
                .expand(&Payload::Base64("aGkK".to_owned()))
                .expect("base64 expands"),
            b"hi\n"
        );
    }

    /// The shorthand is convenience, not a bypass: what reaches the wire is what a
    /// PowerShell prompt hook emits, so the recognizer in acter-term sees no difference.
    #[test]
    fn the_marker_shorthand_expands_to_the_sequence_a_shell_emits() {
        let transcript = SessionTranscript::builtin();
        let expand = |kind, exit_code| {
            transcript
                .expand(&Payload::Marker { kind, exit_code })
                .expect("a marker expands")
        };

        assert_eq!(expand(MarkerKind::PromptStart, None), b"\x1b]133;A\x07");
        assert_eq!(expand(MarkerKind::CommandStart, None), b"\x1b]133;B\x07");
        assert_eq!(expand(MarkerKind::OutputStart, None), b"\x1b]133;C\x07");
        assert_eq!(expand(MarkerKind::CommandEnd, None), b"\x1b]133;D\x07");
        assert_eq!(expand(MarkerKind::CommandEnd, Some(2)), b"\x1b]133;D;2\x07");
    }

    #[test]
    fn a_file_payload_resolves_beside_the_transcript() {
        let transcript =
            SessionTranscript::load(fixture("captured_prompt.json")).expect("the fixture loads");

        let captured = transcript
            .expand(transcript.prompt()[0].payload())
            .expect("the capture resolves");
        assert_eq!(
            captured,
            b"\x1b]133;A\x07PS C:\\acter> \x1b]133;B\x07".to_vec()
        );
    }

    #[test]
    fn a_file_payload_needs_a_transcript_that_came_from_disk() {
        let error =
            one_rule(r#"{ "match": "x", "steps": [{ "payload": { "file": "capture.bin" } }] }"#)
                .expect_err("a string has no folder to resolve against");
        assert!(error.contains("capture.bin"), "got: {error}");
        assert!(error.contains("folder"), "got: {error}");
    }

    #[test]
    fn a_missing_capture_file_is_rejected_when_the_transcript_loads() {
        let error = SessionTranscript::load(fixture("nothing_here.json"))
            .expect_err("a transcript that is not there cannot load");
        assert!(error.contains("could not be read"), "got: {error}");
    }

    #[test]
    fn every_reliability_fixture_loads() {
        for name in [
            "split_marker.json",
            "forged_marker.json",
            "no_end_marker.json",
            "unmarked_session.json",
            "alt_screen.json",
            "device_query.json",
            "captured_prompt.json",
        ] {
            SessionTranscript::load(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn an_inverted_delay_range_is_rejected_loudly() {
        let error = one_rule(
            r#"{
              "match": "x",
              "steps": [
                { "payload": { "text": "a" } },
                { "delay": { "min_ms": 200, "max_ms": 100 }, "payload": { "text": "b" } }
              ]
            }"#,
        )
        .expect_err("min greater than max must be rejected");
        assert!(error.contains("Step 2 of the rule for x"), "got: {error}");
        assert!(
            error.contains("200") && error.contains("100"),
            "got: {error}"
        );
    }

    #[test]
    fn an_unknown_payload_kind_is_rejected() {
        let error = one_rule(r#"{ "match": "x", "steps": [{ "payload": { "sound": "beep" } }] }"#)
            .expect_err("an invented payload kind must be rejected");
        assert!(error.contains("not valid JSON"), "got: {error}");
    }

    #[test]
    fn a_step_that_could_never_happen_is_rejected() {
        let error =
            one_rule(r#"{ "match": "x", "steps": [{ "payload": { "text": "a" }, "repeat": 0 }] }"#)
                .expect_err("a step that repeats zero times is an authoring mistake");
        assert!(error.contains("never happen"), "got: {error}");
    }

    /// An endless step with no delay would spin the emission loop with nothing else ever
    /// getting a turn — including the interrupt that is the only way to stop it.
    #[test]
    fn an_endless_step_with_no_delay_is_rejected() {
        let error = one_rule(
            r#"{ "match": "x", "steps": [{ "payload": { "text": "a" }, "repeat": "forever" }] }"#,
        )
        .expect_err("an endless step must leave room to breathe");
        assert!(error.contains("never pause"), "got: {error}");
    }

    #[test]
    fn an_exit_code_on_a_marker_that_is_not_a_command_end_is_rejected() {
        let error = one_rule(
            r#"{ "match": "x", "steps": [{ "payload": { "marker": { "kind": "A", "exit_code": 0 } } }] }"#,
        )
        .expect_err("only a command end carries an exit code");
        assert!(error.contains("exit code"), "got: {error}");
    }

    #[test]
    fn a_transcript_that_could_not_answer_anything_is_rejected() {
        let error = SessionTranscript::parse(r#"{ "rules": [], "default": { "steps": [] } }"#)
            .expect_err("a transcript with no rules answers nothing");
        assert!(error.contains("no rules"), "got: {error}");

        let error = SessionTranscript::parse(
            r#"{ "rules": [{ "steps": [] }], "default": { "steps": [] } }"#,
        )
        .expect_err("a rule nothing can select is an authoring mistake");
        assert!(error.contains("could ever select it"), "got: {error}");
    }

    #[test]
    fn a_default_rule_that_pretends_to_be_an_ordinary_one_is_rejected() {
        let error = SessionTranscript::parse(
            r#"{ "rules": [{ "match": "x", "steps": [] }],
                 "default": { "match": "y", "steps": [] } }"#,
        )
        .expect_err("the default rule is the one reached by matching nothing");
        assert!(error.contains("default rule"), "got: {error}");

        let error = SessionTranscript::parse(
            r#"{ "rules": [{ "match": "x", "steps": [] }],
                 "default": { "interrupts": true, "steps": [] } }"#,
        )
        .expect_err("an ordinary line must not cancel what is running");
        assert!(error.contains("cancel"), "got: {error}");
    }

    #[test]
    fn malformed_json_is_rejected_with_a_speakable_message() {
        let error = SessionTranscript::parse("{ not json").expect_err("malformed JSON");
        assert!(error.starts_with("The session transcript is not valid JSON."));
    }

    #[test]
    fn equal_bounds_ignore_the_roll_and_unequal_bounds_stay_inside_the_range() {
        let fixed = DelayRange::fixed(250);
        for roll in [0, 1, u64::MAX] {
            assert_eq!(fixed.pick(roll), Duration::from_millis(250));
        }

        let sampled = DelayRange {
            min_ms: 300,
            max_ms: 800,
        };
        for roll in [0, 1, 499, 500, 501, u64::MAX] {
            let picked = sampled.pick(roll).as_millis() as u64;
            assert!((300..=800).contains(&picked), "roll {roll} picked {picked}");
        }
    }

    #[test]
    fn base64_rejects_a_character_that_is_not_base64() {
        let error = decode_base64("aGk!").expect_err("an illegal character must be rejected");
        assert!(error.contains("not a base64 character"), "got: {error}");
    }
}
