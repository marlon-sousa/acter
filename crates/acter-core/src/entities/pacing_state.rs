//! Entity/value: per-command pacing state and the DESIGN-decided pacing numbers.
//! `PacingState` is threaded through `policies::autoread`'s free functions, which
//! return a new state alongside each decision; it never mutates itself and never reads
//! a clock. `PacingConfig` carries every pacing number DESIGN has decided, following
//! A3's fake-script-config precedent of numbers-as-data with a `Default`.

use std::time::Duration;

/// Per-command pacing bookkeeping. Invariants (enforced by `policies::autoread`):
/// `consecutive_auto_reads` never exceeds `PacingConfig::babble_limit`; `patience_fired`
/// latches true at most once per command; `continuous_since` never runs ahead of
/// `last_output_at`. A fresh command starts from `default()`; the caller (a later actor)
/// does not construct or inspect the fields directly, only threads the value through the
/// policy's free functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PacingState {
    pub(crate) consecutive_auto_reads: u32,
    pub(crate) patience_fired: bool,
    pub(crate) babble_tripped: bool,
    /// Offset of the last chunk that carried real text; empty chunks do not move it.
    pub(crate) last_output_at: Duration,
    /// Offset at which the current run of *unread* continuous output began — the last
    /// chunk that arrived after a quiescent gap, or after follow mode read one aloud.
    /// Patience is measured from here, so a command that sits silent and then speaks
    /// does not count that silence as output flowing.
    pub(crate) continuous_since: Duration,
}

/// The pacing thresholds: DESIGN's decided defaults as data (Output pacing / Auto-read
/// threshold). Every DESIGN-decided pacing number appears exactly once, here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacingConfig {
    /// Silence after which accumulated output becomes a chunk (default 0.5s).
    pub quiescence: Duration,
    /// Continuous output with no quiescent gap for this long announces once
    /// (default 10s).
    pub patience: Duration,
    /// Auto-read line cap; exceeding it (or `max_chars`) announces "too big" (default 25).
    pub max_lines: usize,
    /// Auto-read char cap; exceeding it (or `max_lines`) announces "too big" (default 2000).
    pub max_chars: usize,
    /// Consecutive auto-read chunks within one command that trip the babble guard
    /// (default 3).
    pub babble_limit: u32,
    /// The rendering cadence: how long output coalesces before it reaches the buffer
    /// (default 50ms). ARCHITECTURE's number, not DESIGN's — "a short tick (tens of ms)"
    /// so the IPC bridge and DOM never see per-write traffic. It lives here because this
    /// is where pacing numbers live, but it governs the rendering path, which no policy
    /// decides: the buffer loads whenever content arrives.
    pub render_tick: Duration,
}

impl Default for PacingConfig {
    fn default() -> Self {
        Self {
            quiescence: Duration::from_millis(500),
            patience: Duration::from_secs(10),
            max_lines: 25,
            max_chars: 2000,
            babble_limit: 3,
            render_tick: Duration::from_millis(50),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_design() {
        let config = PacingConfig::default();
        assert_eq!(config.quiescence, Duration::from_millis(500));
        assert_eq!(config.patience, Duration::from_secs(10));
        assert_eq!(config.max_lines, 25);
        assert_eq!(config.max_chars, 2000);
        assert_eq!(config.babble_limit, 3);
        assert_eq!(config.render_tick, Duration::from_millis(50));
    }

    #[test]
    fn fresh_state_has_no_history() {
        let state = PacingState::default();
        assert_eq!(state.consecutive_auto_reads, 0);
        assert!(!state.patience_fired);
        assert!(!state.babble_tripped);
        assert_eq!(state.last_output_at, Duration::ZERO);
        assert_eq!(state.continuous_since, Duration::ZERO);
    }
}
