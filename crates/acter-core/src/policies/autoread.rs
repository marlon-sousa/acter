//! Policy: the auto-read/pacing policy. Pure functions over [`PacingState`] and
//! [`PacingConfig`] — elapsed time in (as a `Duration` offset from command start,
//! never a clock read), a decision plus the next wake deadline out. Owns the one place
//! output is measured ([`measure`]) and the one place the size threshold is applied
//! ([`verdict`]); [`on_output`], [`on_wake`], and [`on_command_end`] cover the three
//! moments DESIGN's Output pacing section defines: a chunk arriving, a scheduled
//! deadline firing, and the command ending. The accumulated text buffer stays outside
//! the policy (decision 5): callers pass its measured [`TextSize`], never the bytes.

use std::time::Duration;

use crate::{PacingConfig, PacingState, ReadMode};

/// The size of a span of extracted grid text, trailing whitespace trimmed per line.
/// `chars` counts the trimmed line content plus one separator between each pair of
/// lines, so it never overcounts padding the grid added for column alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSize {
    pub lines: usize,
    pub chars: usize,
}

/// What the pacing policy decided to do about a chunk (or the command's remainder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacingAction {
    /// Nothing to announce: still accumulating, or suppressed by the babble guard.
    None,
    /// Read this chunk aloud under the given verdict.
    Flush(ReadMode),
    /// The patience window elapsed with no quiescent gap: "still running" once.
    StillRunning,
    /// The babble guard tripped: "output continues" once, then quiet for the command.
    OutputContinues,
}

/// The result of one policy call: what to do now, and when to be woken next (a
/// duration from `at`, not an absolute time — the caller owns the clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacingOutcome {
    pub action: PacingAction,
    pub wake_after: Option<Duration>,
}

/// Measures extracted grid text: line count and char count with each line's trailing
/// whitespace trimmed (DESIGN, Auto-read threshold). The one place measurement happens.
pub fn measure(text: &str) -> TextSize {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let separators = lines.len().saturating_sub(1);
    let chars = lines.iter().map(|line| line.chars().count()).sum::<usize>() + separators;
    TextSize {
        lines: lines.len(),
        chars,
    }
}

/// The dual-limit threshold: over `max_lines` or over `max_chars`, whichever is
/// exceeded first.
pub fn verdict(size: TextSize, config: &PacingConfig) -> ReadMode {
    if size.lines > config.max_lines || size.chars > config.max_chars {
        ReadMode::TooBig
    } else {
        ReadMode::Auto
    }
}

/// A chunk of output arrived. In follow mode every non-empty chunk flushes `Auto`
/// immediately, bypassing thresholds and the babble guard entirely (decision: follow
/// mode is an explicit override). Otherwise this call only records that output
/// arrived and asks to be woken at the sooner of the quiescence window (from `at`) and
/// the still-pending patience deadline (from command start) — flushing itself happens
/// in [`on_wake`], once a real gap is observed.
pub fn on_output(
    state: PacingState,
    config: &PacingConfig,
    size: TextSize,
    at: Duration,
    follow_mode: bool,
) -> (PacingState, PacingOutcome) {
    let next = PacingState {
        last_output_at: at,
        ..state
    };

    if follow_mode {
        let action = if size.lines == 0 && size.chars == 0 {
            PacingAction::None
        } else {
            PacingAction::Flush(ReadMode::Auto)
        };
        return (
            next,
            PacingOutcome {
                action,
                wake_after: None,
            },
        );
    }

    let wake_after = if state.patience_fired {
        config.quiescence
    } else {
        config.quiescence.min(config.patience.saturating_sub(at))
    };
    (
        next,
        PacingOutcome {
            action: PacingAction::None,
            wake_after: Some(wake_after),
        },
    )
}

/// A previously requested wake deadline fired. Distinguishes the two reasons a wake
/// can happen: a genuine quiescent gap since the last output (`at - last_output_at >=
/// quiescence`), which flushes the unspoken text under the size policy and the babble
/// guard; or the patience deadline reached while output is still fresh, which fires
/// the "still running" announcement at most once per command.
pub fn on_wake(
    state: PacingState,
    config: &PacingConfig,
    unspoken: TextSize,
    at: Duration,
) -> (PacingState, PacingOutcome) {
    let gap = at.saturating_sub(state.last_output_at);
    if gap >= config.quiescence {
        let (next_state, action) = flush_with_babble_guard(state, config, unspoken);
        return (
            next_state,
            PacingOutcome {
                action,
                wake_after: None,
            },
        );
    }

    let remaining_quiescence = Some(config.quiescence.saturating_sub(gap));
    if !state.patience_fired && at >= config.patience {
        let next_state = PacingState {
            patience_fired: true,
            ..state
        };
        return (
            next_state,
            PacingOutcome {
                action: PacingAction::StillRunning,
                wake_after: remaining_quiescence,
            },
        );
    }

    // Woken before either deadline was actually due (or patience already fired):
    // nothing to do, reschedule for the remaining quiescence window.
    (
        state,
        PacingOutcome {
            action: PacingAction::None,
            wake_after: remaining_quiescence,
        },
    )
}

/// The command ended: flush whatever remains unspoken under the size policy alone.
/// The babble guard does not apply here — it throttles repetitive chunks *within* a
/// running command, not the final reading of what it produced. No wake follows.
pub fn on_command_end(
    state: PacingState,
    config: &PacingConfig,
    unspoken: TextSize,
) -> (PacingState, PacingOutcome) {
    let action = if unspoken.lines == 0 && unspoken.chars == 0 {
        PacingAction::None
    } else {
        PacingAction::Flush(verdict(unspoken, config))
    };
    (
        state,
        PacingOutcome {
            action,
            wake_after: None,
        },
    )
}

/// Applies the size verdict and the babble guard to one mid-command chunk. A `TooBig`
/// verdict always flushes and resets the auto-read streak (it isn't one). An `Auto`
/// verdict flushes and extends the streak, unless the streak has already tripped the
/// guard (silence) or reaches `babble_limit` on this call (the guard trips: emit
/// `OutputContinues` once, count clamped at the limit).
fn flush_with_babble_guard(
    state: PacingState,
    config: &PacingConfig,
    unspoken: TextSize,
) -> (PacingState, PacingAction) {
    if unspoken.lines == 0 && unspoken.chars == 0 {
        return (state, PacingAction::None);
    }

    match verdict(unspoken, config) {
        ReadMode::TooBig => {
            let next = PacingState {
                consecutive_auto_reads: 0,
                ..state
            };
            (next, PacingAction::Flush(ReadMode::TooBig))
        }
        ReadMode::Auto if state.babble_tripped => (state, PacingAction::None),
        ReadMode::Auto => {
            let count = state.consecutive_auto_reads + 1;
            if count >= config.babble_limit {
                let next = PacingState {
                    consecutive_auto_reads: count,
                    babble_tripped: true,
                    ..state
                };
                (next, PacingAction::OutputContinues)
            } else {
                let next = PacingState {
                    consecutive_auto_reads: count,
                    ..state
                };
                (next, PacingAction::Flush(ReadMode::Auto))
            }
        }
        ReadMode::Quiet => unreachable!("verdict() only ever returns Auto or TooBig"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> TextSize {
        TextSize { lines: 1, chars: 5 }
    }

    // --- Thresholds -----------------------------------------------------------

    #[test]
    fn at_the_line_cap_is_auto() {
        let config = PacingConfig::default();
        let text = "a\n".repeat(25);
        let size = measure(&text);
        assert_eq!(size.lines, 25);
        assert_eq!(verdict(size, &config), ReadMode::Auto);
    }

    #[test]
    fn one_over_the_line_cap_is_too_big() {
        let config = PacingConfig::default();
        let text = "a\n".repeat(26);
        let size = measure(&text);
        assert_eq!(size.lines, 26);
        assert_eq!(verdict(size, &config), ReadMode::TooBig);
    }

    #[test]
    fn one_under_the_line_cap_is_auto() {
        let config = PacingConfig::default();
        let text = "a\n".repeat(24);
        assert_eq!(verdict(measure(&text), &config), ReadMode::Auto);
    }

    #[test]
    fn at_the_char_cap_is_auto() {
        let config = PacingConfig::default();
        let text = "x".repeat(2000);
        let size = measure(&text);
        assert_eq!(size.chars, 2000);
        assert_eq!(verdict(size, &config), ReadMode::Auto);
    }

    #[test]
    fn one_over_the_char_cap_is_too_big() {
        let config = PacingConfig::default();
        let text = "x".repeat(2001);
        assert_eq!(verdict(measure(&text), &config), ReadMode::TooBig);
    }

    #[test]
    fn one_under_the_char_cap_is_auto() {
        let config = PacingConfig::default();
        let text = "x".repeat(1999);
        assert_eq!(verdict(measure(&text), &config), ReadMode::Auto);
    }

    #[test]
    fn many_short_lines_trip_the_line_cap_under_the_char_cap() {
        let config = PacingConfig::default();
        let text = "a\n".repeat(30);
        let size = measure(&text);
        assert!(size.chars < config.max_chars);
        assert_eq!(verdict(size, &config), ReadMode::TooBig);
    }

    #[test]
    fn few_long_lines_trip_the_char_cap_under_the_line_cap() {
        let config = PacingConfig::default();
        let text = format!("{}\n{}", "x".repeat(1200), "x".repeat(1200));
        let size = measure(&text);
        assert!(size.lines < config.max_lines);
        assert_eq!(verdict(size, &config), ReadMode::TooBig);
    }

    #[test]
    fn trailing_whitespace_trimming_changes_the_verdict() {
        let config = PacingConfig::default();
        // Raw length is 2010 (over the char cap); trimmed it is 10.
        let text = format!("{}{}", "x".repeat(10), " ".repeat(2000));
        let size = measure(&text);
        assert_eq!(size.chars, 10);
        assert_eq!(verdict(size, &config), ReadMode::Auto);
    }

    // --- Quiescence -------------------------------------------------------------

    #[test]
    fn silence_past_the_window_flushes() {
        let config = PacingConfig::default();
        let (state, out) = on_output(
            PacingState::default(),
            &config,
            small(),
            Duration::ZERO,
            false,
        );
        assert_eq!(out.action, PacingAction::None);
        assert_eq!(out.wake_after, Some(config.quiescence));

        let (_, out) = on_wake(state, &config, small(), config.quiescence);
        assert_eq!(out.action, PacingAction::Flush(ReadMode::Auto));
        assert_eq!(out.wake_after, None);
    }

    #[test]
    fn output_inside_the_window_defers_the_flush_and_extends_the_deadline() {
        let config = PacingConfig::default();
        let (state, _) = on_output(
            PacingState::default(),
            &config,
            small(),
            Duration::ZERO,
            false,
        );
        let (state, _) = on_output(state, &config, small(), Duration::from_millis(300), false);

        // The original deadline (500ms from the first chunk) has passed, but the
        // second chunk pushed last_output_at to 300ms, so the gap is only 200ms:
        // deferred, not flushed.
        let (state, out) = on_wake(state, &config, small(), config.quiescence);
        assert_eq!(out.action, PacingAction::None);
        assert_eq!(out.wake_after, Some(Duration::from_millis(300)));

        // The extended deadline (800ms) does flush.
        let (_, out) = on_wake(state, &config, small(), Duration::from_millis(800));
        assert_eq!(out.action, PacingAction::Flush(ReadMode::Auto));
    }

    // --- Patience -----------------------------------------------------------

    #[test]
    fn continuous_output_fires_patience_once() {
        let config = PacingConfig::default();
        let state = PacingState {
            last_output_at: Duration::from_millis(9900),
            ..PacingState::default()
        };
        let (state, out) = on_wake(state, &config, small(), config.patience);
        assert_eq!(out.action, PacingAction::StillRunning);
        assert!(state.patience_fired);

        // A later wake with the same fresh output does not re-fire.
        let (_, out) = on_wake(
            state,
            &config,
            small(),
            config.patience + Duration::from_millis(100),
        );
        assert_ne!(out.action, PacingAction::StillRunning);
    }

    #[test]
    fn going_quiescent_before_the_window_never_fires_patience() {
        let config = PacingConfig::default();
        let (state, out) = on_wake(
            PacingState::default(),
            &config,
            small(),
            Duration::from_millis(1000),
        );
        assert_eq!(out.action, PacingAction::Flush(ReadMode::Auto));
        assert!(!state.patience_fired);

        // The command ends long before the 10s patience window: it never fires.
        let (state, _) = on_command_end(state, &config, TextSize { lines: 0, chars: 0 });
        assert!(!state.patience_fired);
    }

    #[test]
    fn resuming_after_patience_does_not_refire() {
        let config = PacingConfig::default();
        let fired = PacingState {
            patience_fired: true,
            last_output_at: config.patience,
            ..PacingState::default()
        };
        let (state, _) = on_output(
            fired,
            &config,
            small(),
            config.patience + Duration::from_millis(100),
            false,
        );
        assert!(state.patience_fired);

        let (_, out) = on_wake(
            state,
            &config,
            small(),
            config.patience + config.quiescence + Duration::from_millis(100),
        );
        assert_ne!(out.action, PacingAction::StillRunning);
    }

    // --- Babble guard ---------------------------------------------------------

    #[test]
    fn the_nth_consecutive_auto_read_trips_the_guard_once_then_goes_silent() {
        let config = PacingConfig::default();
        let at = config.quiescence;
        let mut state = PacingState::default();

        let (s, out) = on_wake(state, &config, small(), at);
        assert_eq!(out.action, PacingAction::Flush(ReadMode::Auto));
        state = s;

        let (s, out) = on_wake(state, &config, small(), at);
        assert_eq!(out.action, PacingAction::Flush(ReadMode::Auto));
        state = s;

        // Third consecutive auto-read chunk: babble_limit is 3.
        let (s, out) = on_wake(state, &config, small(), at);
        assert_eq!(out.action, PacingAction::OutputContinues);
        assert!(s.babble_tripped);
        assert_eq!(s.consecutive_auto_reads, config.babble_limit);
        state = s;

        // Fourth: silent, and the counter does not exceed the limit.
        let (s, out) = on_wake(state, &config, small(), at);
        assert_eq!(out.action, PacingAction::None);
        assert_eq!(s.consecutive_auto_reads, config.babble_limit);
    }

    #[test]
    fn a_new_command_resets_the_babble_count() {
        let config = PacingConfig::default();
        let tripped = PacingState {
            consecutive_auto_reads: 3,
            babble_tripped: true,
            ..PacingState::default()
        };
        assert_eq!(
            on_wake(tripped, &config, small(), config.quiescence)
                .1
                .action,
            PacingAction::None
        );

        // A fresh PacingState (new command) is not tripped.
        let fresh = PacingState::default();
        assert_eq!(
            on_wake(fresh, &config, small(), config.quiescence).1.action,
            PacingAction::Flush(ReadMode::Auto)
        );
    }

    #[test]
    fn follow_mode_suppresses_the_babble_guard_entirely() {
        let config = PacingConfig::default();
        let mut state = PacingState::default();
        for _ in 0..5 {
            let (next, out) = on_output(state, &config, small(), Duration::ZERO, true);
            assert_eq!(out.action, PacingAction::Flush(ReadMode::Auto));
            assert_eq!(out.wake_after, None);
            state = next;
        }
        assert_eq!(state.consecutive_auto_reads, 0);
        assert!(!state.babble_tripped);
    }

    // --- Command end ------------------------------------------------------------

    #[test]
    fn command_end_flushes_the_remainder_under_the_size_policy() {
        let config = PacingConfig::default();
        let (_, out) = on_command_end(PacingState::default(), &config, small());
        assert_eq!(out.action, PacingAction::Flush(ReadMode::Auto));

        let big = measure(&"a\n".repeat(30));
        let (_, out) = on_command_end(PacingState::default(), &config, big);
        assert_eq!(out.action, PacingAction::Flush(ReadMode::TooBig));
    }

    #[test]
    fn command_end_inside_a_quiescence_window_still_flushes() {
        let config = PacingConfig::default();
        // Output just arrived; the quiescence window has not elapsed. The command
        // ends right away regardless.
        let (state, _) = on_output(
            PacingState::default(),
            &config,
            small(),
            Duration::ZERO,
            false,
        );
        let (_, out) = on_command_end(state, &config, small());
        assert_eq!(out.action, PacingAction::Flush(ReadMode::Auto));
    }

    #[test]
    fn command_end_with_nothing_unspoken_emits_no_action() {
        let config = PacingConfig::default();
        let (_, out) = on_command_end(
            PacingState::default(),
            &config,
            TextSize { lines: 0, chars: 0 },
        );
        assert_eq!(out.action, PacingAction::None);
        assert_eq!(out.wake_after, None);
    }
}
