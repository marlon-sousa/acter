//! Entity/value: shared IPC protocol value types — identity, correlation, and the
//! verdict/state enums carried across the frontend wire. Pure data, no behavior.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Identifies a session (one per tab). A distinct named type so it can never be
/// confused with a [`CommandId`]; serializes as a bare integer. `u32` (not `u64`)
/// because it maps to a JS `number` without precision loss and a session/tab counter
/// never approaches four billion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct SessionId(pub u32);

/// Correlation id tying a submitted command to every event about it. `submit_command`
/// returns one; every later event about that command carries it. `u32` for the same
/// reason as [`SessionId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct CommandId(pub u32);

/// Process exit status. Nonzero is a failure, announced distinctly from success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct ExitCode(pub i32);

/// The read verdict computed backend-side by the pacing policy and carried on every
/// announcement-bearing event. The frontend obeys it and never re-measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ReadMode {
    /// Small enough to read aloud automatically via the live region.
    Auto,
    /// Over threshold: announced as "too big to read"; a beep signals completion.
    TooBig,
    /// Suppressed (e.g. the babble guard tripped); accumulates silently in the buffer.
    Quiet,
}

/// Rendering mode over the one live session. Phase 1 only ever emits
/// [`Mode::NonInteractive`]; the interactive variant is defined so Phase 2 is additive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Mode {
    /// Conversational, screen-reader-native mode: local edit field + results buffer.
    NonInteractive,
    /// Full terminal pass-through for ncurses/full-screen programs.
    Interactive,
}

/// Transport connection state. The local transport is always [`ConnectionState::Connected`]
/// in Phase 1; the other states are exercised once SSH lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ConnectionState {
    Connected,
    Reconnecting,
    Disconnected,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn newtypes_are_transparent_scalars_on_the_wire() {
        assert_eq!(serde_json::to_value(SessionId(7)).unwrap(), json!(7));
        assert_eq!(serde_json::to_value(CommandId(42)).unwrap(), json!(42));
        assert_eq!(serde_json::to_value(ExitCode(-1)).unwrap(), json!(-1));
    }

    #[test]
    fn newtypes_round_trip() {
        for id in [SessionId(0), SessionId(u32::MAX)] {
            let back: SessionId =
                serde_json::from_value(serde_json::to_value(id).unwrap()).unwrap();
            assert_eq!(id, back);
        }
        for code in [ExitCode(0), ExitCode(1), ExitCode(i32::MIN)] {
            let back: ExitCode =
                serde_json::from_value(serde_json::to_value(code).unwrap()).unwrap();
            assert_eq!(code, back);
        }
    }

    #[test]
    fn unit_enums_serialize_as_their_variant_name() {
        assert_eq!(
            serde_json::to_value(ReadMode::TooBig).unwrap(),
            json!("TooBig")
        );
        assert_eq!(
            serde_json::to_value(Mode::NonInteractive).unwrap(),
            json!("NonInteractive")
        );
        assert_eq!(
            serde_json::to_value(ConnectionState::Connected).unwrap(),
            json!("Connected")
        );
    }

    #[test]
    fn unit_enums_round_trip_every_variant() {
        for mode in [ReadMode::Auto, ReadMode::TooBig, ReadMode::Quiet] {
            let back: ReadMode =
                serde_json::from_value(serde_json::to_value(mode).unwrap()).unwrap();
            assert_eq!(mode, back);
        }
        for mode in [Mode::NonInteractive, Mode::Interactive] {
            let back: Mode = serde_json::from_value(serde_json::to_value(mode).unwrap()).unwrap();
            assert_eq!(mode, back);
        }
        for state in [
            ConnectionState::Connected,
            ConnectionState::Reconnecting,
            ConnectionState::Disconnected,
        ] {
            let back: ConnectionState =
                serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
            assert_eq!(state, back);
        }
    }
}
