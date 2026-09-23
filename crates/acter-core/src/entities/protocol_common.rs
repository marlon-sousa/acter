//! Entity/value: shared IPC protocol value types — identity, correlation, and the
//! state enums carried across the frontend wire.

use serde::{Deserialize, Serialize};
use specta::Type;

/// `u32` so it fits a JavaScript number exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct SessionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct CommandId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct ExitCode(pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Mode {
    /// Local edit field and results buffer.
    NonInteractive,
    /// Full terminal pass-through.
    Interactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ConnectionState {
    /// Started, and nothing has arrived from the far end yet.
    Connecting,
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
