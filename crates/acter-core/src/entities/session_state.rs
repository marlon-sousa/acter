//! Entity/value: session-scoped state — rendering mode, shell-integration status, and
//! which screen (normal or alternate) is on display. Transitions return a new state;
//! per-command block lifecycle belongs to the boundary tracker (B2), not here — the two
//! touch but do not overlap (decision 9).

use crate::Mode;

/// Whether OSC 133 markers have been observed for this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integration {
    /// No markers seen yet; still within the startup grace period.
    Pending,
    /// Markers observed: command boundaries are trustworthy.
    Integrated,
    /// No markers within the grace period: every command degrades to patience-only
    /// behavior until markers are observed (decision 8 allows recovery).
    Unintegrated,
}

/// Which screen the terminal is currently rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Normal,
    Alternate,
}

/// Session-scoped facts: what kind of session this is and what is on screen.
/// Invariants: [`Integration::Pending`] resolves to `Integrated` or `Unintegrated`
/// exactly once; markers observed after `Unintegrated` recover to `Integrated`
/// (decision 8); alt-screen transitions are idempotent — entering twice is one entry,
/// because a program redrawing does not mean it re-entered (decision 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionState {
    pub mode: Mode,
    pub integration: Integration,
    pub screen: Screen,
}

impl SessionState {
    /// The state of a freshly attached session: given mode, integration pending,
    /// normal screen.
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            integration: Integration::Pending,
            screen: Screen::Normal,
        }
    }

    /// OSC 133 markers observed. Resolves `Pending`, and recovers `Unintegrated`
    /// (decision 8); an already-`Integrated` session is unaffected.
    pub fn markers_observed(self) -> Self {
        match self.integration {
            Integration::Pending | Integration::Unintegrated => Self {
                integration: Integration::Integrated,
                ..self
            },
            Integration::Integrated => self,
        }
    }

    /// The startup grace period elapsed with no markers observed. Resolves `Pending`
    /// to `Unintegrated`; a session already resolved either way is unaffected.
    pub fn grace_period_expired(self) -> Self {
        match self.integration {
            Integration::Pending => Self {
                integration: Integration::Unintegrated,
                ..self
            },
            Integration::Integrated | Integration::Unintegrated => self,
        }
    }

    /// Toggle between non-interactive and interactive rendering (Ctrl+Shift+E).
    pub fn mode_toggled(self) -> Self {
        let mode = match self.mode {
            Mode::NonInteractive => Mode::Interactive,
            Mode::Interactive => Mode::NonInteractive,
        };
        Self { mode, ..self }
    }

    /// A program entered the alternate screen. Idempotent: a redraw does not mean it
    /// re-entered.
    pub fn alt_screen_entered(self) -> Self {
        Self {
            screen: Screen::Alternate,
            ..self
        }
    }

    /// The alternate screen was left; non-interactive rendering resumes. Idempotent.
    pub fn alt_screen_left(self) -> Self {
        Self {
            screen: Screen::Normal,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_resolves_to_integrated_on_markers() {
        let state = SessionState::new(Mode::NonInteractive).markers_observed();
        assert_eq!(state.integration, Integration::Integrated);
    }

    #[test]
    fn pending_resolves_to_unintegrated_on_grace_period() {
        let state = SessionState::new(Mode::NonInteractive).grace_period_expired();
        assert_eq!(state.integration, Integration::Unintegrated);
    }

    #[test]
    fn unintegrated_recovers_to_integrated_on_markers() {
        let state = SessionState::new(Mode::NonInteractive)
            .grace_period_expired()
            .markers_observed();
        assert_eq!(state.integration, Integration::Integrated);
    }

    #[test]
    fn integrated_is_unaffected_by_late_grace_period_expiry() {
        let state = SessionState::new(Mode::NonInteractive)
            .markers_observed()
            .grace_period_expired();
        assert_eq!(state.integration, Integration::Integrated);
    }

    #[test]
    fn integrated_is_unaffected_by_repeated_markers() {
        let state = SessionState::new(Mode::NonInteractive)
            .markers_observed()
            .markers_observed();
        assert_eq!(state.integration, Integration::Integrated);
    }

    #[test]
    fn unintegrated_stays_unintegrated_on_repeated_grace_period_expiry() {
        let state = SessionState::new(Mode::NonInteractive)
            .grace_period_expired()
            .grace_period_expired();
        assert_eq!(state.integration, Integration::Unintegrated);
    }

    #[test]
    fn mode_toggles_both_ways() {
        let state = SessionState::new(Mode::NonInteractive);
        let toggled = state.mode_toggled();
        assert_eq!(toggled.mode, Mode::Interactive);
        assert_eq!(toggled.mode_toggled().mode, Mode::NonInteractive);
    }

    #[test]
    fn alt_screen_entering_twice_is_one_entry() {
        let state = SessionState::new(Mode::NonInteractive)
            .alt_screen_entered()
            .alt_screen_entered();
        assert_eq!(state.screen, Screen::Alternate);
    }

    #[test]
    fn alt_screen_round_trips() {
        let state = SessionState::new(Mode::NonInteractive)
            .alt_screen_entered()
            .alt_screen_left();
        assert_eq!(state.screen, Screen::Normal);
    }

    #[test]
    fn leaving_alt_screen_when_already_normal_is_idempotent() {
        let state = SessionState::new(Mode::NonInteractive).alt_screen_left();
        assert_eq!(state.screen, Screen::Normal);
    }
}
