//! Entity/value: per-command pacing state and the DESIGN-decided pacing numbers.
//! `PacingState` is threaded through `policies::autoread`'s free functions, which
//! return a new state alongside each decision; it never mutates itself and never reads
//! a clock.

use std::time::Duration;

/// `consecutive_auto_reads` never exceeds `PacingConfig::babble_limit`; `patience_fired`
/// latches true at most once per command; `continuous_since` never runs ahead of
/// `last_output_at`. The caller does not construct or inspect the fields directly, only
/// threads the value through the policy's free functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PacingState {
    pub(crate) consecutive_auto_reads: u32,
    pub(crate) patience_fired: bool,
    pub(crate) babble_tripped: bool,
    /// Offset of the last chunk that carried real text; empty chunks do not move it.
    pub(crate) last_output_at: Duration,
    /// Offset at which the current run of *unread* continuous output began — the last
    /// chunk that arrived after a quiescent gap, or after follow mode read one aloud.
    /// Patience is measured from here, so silence before output flows is never counted
    /// as output flowing.
    pub(crate) continuous_since: Duration,
}

/// Every DESIGN-decided pacing number appears exactly once, here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacingConfig {
    /// Silence after which accumulated output becomes a chunk.
    pub quiescence: Duration,
    /// Continuous output with no quiescent gap for this long announces once.
    pub patience: Duration,
    /// Auto-read line cap; exceeding it (or `max_chars`) announces "too big".
    pub max_lines: usize,
    /// Auto-read char cap; exceeding it (or `max_lines`) announces "too big".
    pub max_chars: usize,
    /// Consecutive auto-read chunks within one command that trip the babble guard.
    pub babble_limit: u32,
    /// How long a session waits for its first shell-integration marker before it is
    /// flagged unintegrated. Chosen to cover shell startup, since the injected snippet
    /// emits markers on the first prompt; a false `Unintegrated` degrades every command
    /// in the session, while a late detection costs only one command's boundaries and
    /// then recovers ([`SessionState::markers_observed`](crate::SessionState::markers_observed)
    /// upgrades from `Unintegrated`).
    pub integration_grace: Duration,
    /// How long a keystroke's answer coalesces before the far end's line reaches the
    /// listener, while the far end owns the line.
    ///
    /// A coalescing gap added on top of the far end's own round trip, not a latency
    /// budget. NVDA polls the caret every 10ms up to `caretMoveTimeoutMs` (100ms
    /// default, user-raisable to 2000 in Advanced settings; `source/editableText.py`,
    /// `EditableText._caretMovementScriptHelper`) and on timeout speaks the caret that
    /// did not move. Measured with `acter-transports/examples/latency.rs`: `bash` under
    /// WSL answered left in 1ms, Home in 0ms, up in 3ms, Backspace in 4ms; Windows
    /// PowerShell answered all four in 0ms; `cmd.exe` in 0 to 1ms.
    pub far_end_settle: Duration,
    /// The rendering cadence: how long output coalesces before it reaches the buffer.
    /// ARCHITECTURE's number, not a policy decision: it governs the rendering path, and
    /// the buffer loads whenever content arrives.
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
