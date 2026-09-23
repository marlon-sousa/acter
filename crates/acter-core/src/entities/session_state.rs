//! Entity/value: session-scoped state — rendering mode, shell-integration status, and
//! which screen (normal or alternate) is on display.

use crate::Mode;

/// Whether OSC 133 markers have been observed for this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integration {
    /// Within the startup grace period, no marker seen yet.
    Pending,
    Integrated,
    /// Commands fall back to the patience timer until a marker arrives.
    Unintegrated,
}

impl Integration {
    pub fn markers_observed(self) -> Self {
        match self {
            Self::Pending | Self::Unintegrated => Self::Integrated,
            Self::Integrated => self,
        }
    }

    pub fn grace_period_expired(self) -> Self {
        match self {
            Self::Pending => Self::Unintegrated,
            Self::Integrated | Self::Unintegrated => self,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Normal,
    Alternate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionState {
    pub mode: Mode,
    pub integration: Integration,
    pub screen: Screen,
}

impl SessionState {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            integration: Integration::Pending,
            screen: Screen::Normal,
        }
    }

    pub fn markers_observed(self) -> Self {
        Self {
            integration: self.integration.markers_observed(),
            ..self
        }
    }

    pub fn grace_period_expired(self) -> Self {
        Self {
            integration: self.integration.grace_period_expired(),
            ..self
        }
    }

    pub fn mode_toggled(self) -> Self {
        let mode = match self.mode {
            Mode::NonInteractive => Mode::Interactive,
            Mode::Interactive => Mode::NonInteractive,
        };
        Self { mode, ..self }
    }

    pub fn alt_screen_entered(self) -> Self {
        Self {
            screen: Screen::Alternate,
            ..self
        }
    }

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
    fn the_bare_integration_transitions_match_the_session_state_ones() {
        let states = [
            Integration::Pending,
            Integration::Integrated,
            Integration::Unintegrated,
        ];
        for integration in states {
            let state = SessionState {
                integration,
                ..SessionState::new(Mode::NonInteractive)
            };
            assert_eq!(
                state.markers_observed().integration,
                integration.markers_observed()
            );
            assert_eq!(
                state.grace_period_expired().integration,
                integration.grace_period_expired()
            );
        }
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
