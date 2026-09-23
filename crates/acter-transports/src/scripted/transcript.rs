//! Entity/value: the session transcript — the JSON a scripted session is played from,
//! its validation, and the expansion of a step's payload into the exact bytes that go
//! on the wire.

use std::fs::{read, read_to_string};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

const OSC133: &[u8] = b"\x1b]133;";
const BEL: u8 = 0x07;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTranscript {
    #[serde(default)]
    on_start: Vec<Step>,
    rules: Vec<Rule>,
    default: Rule,
    /// `None` for a transcript parsed from a string, which rejects every `file` payload.
    #[serde(skip)]
    base: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    /// `None` on the default rule.
    #[serde(rename = "match", default)]
    line: Option<String>,
    #[serde(default)]
    interrupts: bool,
    steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Step {
    #[serde(default)]
    delay: DelayRange,
    payload: Payload,
    #[serde(default)]
    repeat: Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DelayRange {
    min_ms: u64,
    max_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub(crate) enum Repeat {
    Times(u32),
    Endless(Endless),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Endless {
    Forever,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Payload {
    Text(String),
    Marker {
        kind: MarkerKind,
        #[serde(default)]
        exit_code: Option<i32>,
    },
    Base64(String),
    File(String),
}

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

    pub(crate) const fn is_instant(self) -> bool {
        self.max_ms == 0
    }

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
    pub fn builtin() -> Self {
        Self::parse(include_str!("default_transcript.json"))
            .expect("the built-in transcript must parse")
    }

    /// Rejects a `file` payload; the error is a speakable sentence.
    pub fn parse(json: &str) -> Result<Self, String> {
        let transcript: Self = serde_json::from_str(json)
            .map_err(|e| format!("The session transcript is not valid JSON. {e}"))?;
        transcript.validate()?;
        Ok(transcript)
    }

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

    pub(crate) fn prompt(&self) -> &[Step] {
        &self.on_start
    }

    pub(crate) fn rule_for(&self, line: &str) -> &Rule {
        self.rules
            .iter()
            .find(|rule| rule.line.as_deref() == Some(line))
            .unwrap_or(&self.default)
    }

    pub(crate) fn interrupts(&self, line: &str) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.line.as_deref() == Some(line) && rule.interrupts)
    }

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
            "small", "big", "fail", "slow", "forever", "nano", "tail", "burst", "speech",
        ] {
            assert_ne!(
                transcript.rule_for(scenario),
                default,
                "the built-in transcript must answer {scenario}"
            );
        }
    }

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
            "forged_marker.json",
            "no_end_marker.json",
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
