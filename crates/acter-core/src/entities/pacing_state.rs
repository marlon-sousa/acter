//! Entity/value: per-command pacing state and the pacing numbers `policies::autoread`
//! reads.

use std::time::Duration;

/// Invariants `policies::autoread` alone enforces: `consecutive_auto_reads` never exceeds
/// `babble_limit`, `patience_fired` latches once per command, and `continuous_since` never
/// passes `last_output_at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PacingState {
    pub(crate) consecutive_auto_reads: u32,
    pub(crate) patience_fired: bool,
    pub(crate) babble_tripped: bool,
    /// Offset of the last chunk that carried real text; empty chunks do not move it.
    pub(crate) last_output_at: Duration,
    /// Where the current run of unread output began; patience is measured from here.
    pub(crate) continuous_since: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacingConfig {
    /// Silence after which accumulated output becomes a chunk.
    pub quiescence: Duration,
    /// Continuous output with no quiescent gap for this long announces once.
    pub patience: Duration,
    /// Exceeding this or `max_chars` announces "too big".
    pub max_lines: usize,
    pub max_chars: usize,
    /// Consecutive auto-read chunks within one command that trip the babble guard.
    pub babble_limit: u32,
    /// How long a session waits for its first shell-integration marker.
    pub integration_grace: Duration,
    /// Added after the far end's own echo while it owns the line; see
    /// acter-transports' examples/latency.rs.
    pub far_end_settle: Duration,
    /// How long output coalesces before it reaches the buffer.
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
            integration_grace: Duration::from_secs(5),
            far_end_settle: Duration::from_millis(30),
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
        assert_eq!(config.far_end_settle, Duration::from_millis(30));
        assert!(
            config.far_end_settle < config.quiescence,
            "a keystroke a listener is waiting on is not paced like a transcript"
        );
        assert_eq!(config.integration_grace, Duration::from_secs(5));
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
