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
    /// How long a session waits for its first shell-integration marker before it is
    /// flagged unintegrated (default 5s).
    ///
    /// DESIGN decides the grace period exists and gives no number. Five seconds because
    /// it only has to cover shell startup — the injected snippet emits markers on the
    /// first prompt — but PowerShell profile loading routinely takes seconds, and the
    /// asymmetry is sharp: a false `Unintegrated` degrades every command in the
    /// session, while a late detection costs one command's boundaries and then recovers
    /// ([`SessionState::markers_observed`](crate::SessionState::markers_observed)
    /// upgrades from `Unintegrated`). Of every number in this struct it is the one most
    /// likely to be retuned by NVDA evidence (spec B6, decision 9).
    pub integration_grace: Duration,
    /// How long a keystroke's answer coalesces before the far end's line reaches the
    /// listener, while the far end owns the line (default 30ms).
    ///
    /// **It is a coalescing gap and not a latency budget**, and the difference is the whole
    /// of roadmap 28.1. It is added on top of the far end's own round trip, on every
    /// endpoint, always — so the right value is the smallest one that still holds a redraw
    /// together, never the largest one that fits in some window.
    ///
    /// **The window it has to fit inside is the screen reader's, and it is not ours.** NVDA
    /// answers an arrow key by sending it on and then polling the caret every 10ms until it
    /// moves or `caretMoveTimeoutMs` elapses — 100ms by default, and the user's to raise to
    /// 2000 in Advanced settings (`source/editableText.py`,
    /// `EditableText._caretMovementScriptHelper`). On timeout it speaks the caret that did
    /// not move, so a late answer is not silence, it is the *previous* answer said again.
    /// That is what a listener met before this number existed: `quiescence` was doing this
    /// job at 500ms, five times the reader's patience.
    ///
    /// **Thirty milliseconds because the far end answers in single digits.** Measured
    /// 2026-09-02 with `acter-transports/examples/latency.rs`, timestamping every line and
    /// cursor change from the byte going out: `bash` under WSL answered left in 1ms, Home in
    /// 0ms, up in 3ms and Backspace in 4ms; Windows PowerShell answered all four in 0ms;
    /// `cmd.exe` in 0 to 1ms. Every one arrived in a single batch, first change and settled
    /// at the same instant. Thirty is roughly seven times the worst of those as headroom for
    /// a redraw split across reads, and it leaves seventy of the reader's hundred for the
    /// round trip itself — which is the part that varies with the endpoint and that no
    /// number here can absorb.
    pub far_end_settle: Duration,
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
