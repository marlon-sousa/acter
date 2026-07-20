//! Entity/value: the fake script config — the numbers behind every scripted scenario
//! (decision 8). Pure data plus parsing and range validation; no time, no randomness,
//! no I/O. The service reads these values and samples the delay ranges; the container
//! loads an optional JSON override. `Default` carries the human-scale manual-testing
//! numbers from the spec's scenario table, so `tauri dev` needs no file.
//!
//! Only *numbers* live here — delays, chunk sizes, line counts, iteration counts,
//! exit codes. Scenario *shapes* (event order, phrasing) stay in the service as code
//! (decision 8's second scope line): this is a parameter set, not a step-scripting DSL.

use serde::Deserialize;

/// A delay expressed as an inclusive millisecond range. Equal bounds are a fixed delay
/// (E2E determinism); unequal bounds are sampled per use for organic manual pacing.
/// Invariant: `min_ms <= max_ms`, checked by [`FakeScript::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct DelayRange {
    pub(crate) min_ms: u64,
    pub(crate) max_ms: u64,
}

impl DelayRange {
    /// A fixed delay (both bounds equal), the common manual-testing case.
    pub(crate) const fn fixed(ms: u64) -> Self {
        Self {
            min_ms: ms,
            max_ms: ms,
        }
    }

    /// An inclusive range sampled per use.
    pub(crate) const fn range(min_ms: u64, max_ms: u64) -> Self {
        Self { min_ms, max_ms }
    }
}

/// `small`: the plain auto-read case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct SmallScript {
    pub(crate) output_delay: DelayRange,
}

/// `big`: the too-big announcement plus the completion beep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct BigScript {
    pub(crate) output_delay: DelayRange,
    pub(crate) line_count: u32,
    pub(crate) finish_delay: DelayRange,
}

/// `fail`: the failure announcement with a nonzero exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct FailScript {
    pub(crate) output_delay: DelayRange,
    pub(crate) exit_code: i32,
}

/// `slow`: phase-by-phase narration, three chunks paced by one interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct SlowScript {
    pub(crate) chunk_delay: DelayRange,
}

/// `forever`: patience announcement, then silent accumulation with no finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct ForeverScript {
    /// Interval before each of the two opening auto-read chunks.
    pub(crate) chunk_delay: DelayRange,
    /// Delay after the second chunk before the patience announcement.
    pub(crate) patience_delay: DelayRange,
    /// Interval between the endless quiet chunks.
    pub(crate) quiet_interval: DelayRange,
}

/// `nano`: the phase-1 alt-screen announcements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct NanoScript {
    pub(crate) enter_delay: DelayRange,
    pub(crate) leave_delay: DelayRange,
}

/// `tail`: the buffer-reading experiment — small auto-read chunks under a live stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct TailScript {
    pub(crate) iterations: u32,
    pub(crate) interval: DelayRange,
}

/// `speech`: one long auto-read phrase, deliberately longer than the live region's
/// clear delay, for checking that emptying the region never truncates speech the
/// screen reader has already queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct SpeechScript {
    pub(crate) output_delay: DelayRange,
    /// Numbered words between the opening and closing markers. The phrase is counted
    /// so a truncation is audible as the exact word it stopped at.
    pub(crate) word_count: u32,
}

/// `burst`: a too-big flood followed by a trickle of small auto-read chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct BurstScript {
    pub(crate) flood_delay: DelayRange,
    pub(crate) flood_lines: u32,
    pub(crate) iterations: u32,
    pub(crate) interval: DelayRange,
}

/// The whole fake script config: one entry per scenario. A missing scenario in an
/// override file falls back to its built-in default (`#[serde(default)]`), so a
/// hand-edited file can tune one scenario without restating the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FakeScript {
    #[serde(default)]
    pub(crate) small: SmallScript,
    #[serde(default)]
    pub(crate) big: BigScript,
    #[serde(default)]
    pub(crate) fail: FailScript,
    #[serde(default)]
    pub(crate) slow: SlowScript,
    #[serde(default)]
    pub(crate) forever: ForeverScript,
    #[serde(default)]
    pub(crate) nano: NanoScript,
    #[serde(default)]
    pub(crate) tail: TailScript,
    #[serde(default)]
    pub(crate) burst: BurstScript,
    #[serde(default)]
    pub(crate) speech: SpeechScript,
}

// Built-in defaults: the human-scale manual-testing numbers from the scenario table.

impl Default for SmallScript {
    fn default() -> Self {
        Self {
            output_delay: DelayRange::fixed(100),
        }
    }
}

impl Default for BigScript {
    fn default() -> Self {
        Self {
            output_delay: DelayRange::fixed(100),
            line_count: 40,
            finish_delay: DelayRange::fixed(1500),
        }
    }
}

impl Default for FailScript {
    fn default() -> Self {
        Self {
            output_delay: DelayRange::fixed(100),
            exit_code: 2,
        }
    }
}

impl Default for SlowScript {
    fn default() -> Self {
        Self {
            chunk_delay: DelayRange::fixed(1000),
        }
    }
}

impl Default for ForeverScript {
    fn default() -> Self {
        Self {
            chunk_delay: DelayRange::fixed(1000),
            patience_delay: DelayRange::fixed(2000),
            quiet_interval: DelayRange::fixed(2000),
        }
    }
}

impl Default for NanoScript {
    fn default() -> Self {
        Self {
            enter_delay: DelayRange::fixed(300),
            leave_delay: DelayRange::fixed(5000),
        }
    }
}

impl Default for TailScript {
    fn default() -> Self {
        Self {
            iterations: 10,
            interval: DelayRange::range(3000, 8000),
        }
    }
}

impl Default for BurstScript {
    fn default() -> Self {
        Self {
            flood_delay: DelayRange::fixed(300),
            flood_lines: 60,
            iterations: 4,
            interval: DelayRange::range(3000, 8000),
        }
    }
}

impl Default for SpeechScript {
    fn default() -> Self {
        Self {
            output_delay: DelayRange::fixed(100),
            // Sixty numbered words is roughly twenty seconds of speech — comfortably
            // longer than the frontend's live-region clear delay, which is the point.
            word_count: 60,
        }
    }
}

impl FakeScript {
    /// Every delay range in the config, for validation. Order is irrelevant — only
    /// the invariant matters — but naming each range makes rejection messages precise.
    fn ranges(&self) -> [(&'static str, DelayRange); 13] {
        [
            ("small.output_delay", self.small.output_delay),
            ("big.output_delay", self.big.output_delay),
            ("big.finish_delay", self.big.finish_delay),
            ("fail.output_delay", self.fail.output_delay),
            ("slow.chunk_delay", self.slow.chunk_delay),
            ("forever.chunk_delay", self.forever.chunk_delay),
            ("forever.patience_delay", self.forever.patience_delay),
            ("forever.quiet_interval", self.forever.quiet_interval),
            ("nano.enter_delay", self.nano.enter_delay),
            ("nano.leave_delay", self.nano.leave_delay),
            ("tail.interval", self.tail.interval),
            ("burst.interval", self.burst.interval),
            ("speech.output_delay", self.speech.output_delay),
        ]
    }

    /// Rejects a range whose `min_ms` exceeds its `max_ms`. Returns a speakable
    /// message naming the offending field (this surfaces as a loud startup error).
    fn validate(&self) -> Result<(), String> {
        for (name, range) in self.ranges() {
            if range.min_ms > range.max_ms {
                return Err(format!(
                    "fake script config: {name} has min_ms {} greater than max_ms {}",
                    range.min_ms, range.max_ms
                ));
            }
        }
        Ok(())
    }
}

/// Parses a fake script config from JSON and validates its ranges. A parse or
/// validation failure returns a speakable message; the container turns it into a loud
/// startup error rather than a silent fallback (spec, container deliverable).
pub(crate) fn parse(json: &str) -> Result<FakeScript, String> {
    let script: FakeScript = serde_json::from_str(json)
        .map_err(|e| format!("fake script config is not valid JSON: {e}"))?;
    script.validate()?;
    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_scenario_table() {
        let script = FakeScript::default();
        assert_eq!(script.small.output_delay, DelayRange::fixed(100));
        assert_eq!(script.big.line_count, 40);
        assert_eq!(script.big.finish_delay, DelayRange::fixed(1500));
        assert_eq!(script.fail.exit_code, 2);
        assert_eq!(script.tail.iterations, 10);
        assert_eq!(script.tail.interval, DelayRange::range(3000, 8000));
        assert_eq!(script.burst.flood_lines, 60);
        assert_eq!(script.burst.iterations, 4);
    }

    #[test]
    fn parses_a_full_config_file() {
        let json = r#"{
            "small": { "output_delay": { "min_ms": 5, "max_ms": 5 } },
            "big": {
                "output_delay": { "min_ms": 5, "max_ms": 5 },
                "line_count": 12,
                "finish_delay": { "min_ms": 10, "max_ms": 10 }
            },
            "fail": { "output_delay": { "min_ms": 5, "max_ms": 5 }, "exit_code": 3 },
            "slow": { "chunk_delay": { "min_ms": 5, "max_ms": 5 } },
            "forever": {
                "chunk_delay": { "min_ms": 5, "max_ms": 5 },
                "patience_delay": { "min_ms": 5, "max_ms": 5 },
                "quiet_interval": { "min_ms": 5, "max_ms": 5 }
            },
            "nano": {
                "enter_delay": { "min_ms": 5, "max_ms": 5 },
                "leave_delay": { "min_ms": 5, "max_ms": 5 }
            },
            "tail": { "iterations": 2, "interval": { "min_ms": 5, "max_ms": 5 } },
            "burst": {
                "flood_delay": { "min_ms": 5, "max_ms": 5 },
                "flood_lines": 7,
                "iterations": 2,
                "interval": { "min_ms": 5, "max_ms": 5 }
            }
        }"#;
        let script = parse(json).expect("full config should parse");
        assert_eq!(script.big.line_count, 12);
        assert_eq!(script.fail.exit_code, 3);
        assert_eq!(script.tail.iterations, 2);
        assert_eq!(script.burst.flood_lines, 7);
        assert_eq!(script.small.output_delay, DelayRange::fixed(5));
    }

    #[test]
    fn missing_scenarios_fall_back_to_defaults() {
        // A partial override tunes one scenario; the rest keep their built-in values.
        let script =
            parse(r#"{ "tail": { "iterations": 3, "interval": { "min_ms": 1, "max_ms": 2 } } }"#)
                .expect("partial config should parse");
        assert_eq!(script.tail.iterations, 3);
        assert_eq!(script.big, BigScript::default());
        assert_eq!(script.small, SmallScript::default());
    }

    #[test]
    fn empty_object_is_all_defaults() {
        assert_eq!(
            parse("{}").expect("empty object parses"),
            FakeScript::default()
        );
    }

    #[test]
    fn rejects_an_inverted_range_with_a_speakable_message() {
        let json = r#"{ "small": { "output_delay": { "min_ms": 200, "max_ms": 100 } } }"#;
        let err = parse(json).expect_err("min greater than max must be rejected");
        assert!(
            err.contains("small.output_delay") && err.contains("200") && err.contains("100"),
            "message should name the field and both bounds, got: {err}"
        );
    }

    #[test]
    fn rejects_malformed_json() {
        let err = parse("{ not json").expect_err("malformed JSON must be rejected");
        assert!(err.contains("valid JSON"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = parse(r#"{ "nonsense": {} }"#).expect_err("unknown scenario must be rejected");
        assert!(err.contains("valid JSON"), "got: {err}");
    }
}
