//! Entity/value: return payloads for the frontend-to-backend command (invoke) surface.
//!
//! An invoke never waits on the shell (ARCHITECTURE, IPC rules): `submit_command`
//! returns immediately with the correlation id every later event carries. Phase-1
//! invoke *arguments* are primitives (`session_id`, `line`, `cols`, `rows`) passed
//! straight to routers in A3, so the only named payload here is the ack. Completion
//! (A4) and session-snapshot (A3) payloads are defined with the domains that build them.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::CommandId;

/// The immediate return of `submit_command`: the id correlating this submission with
/// its `CommandStarted` / `Output` / `CommandFinished` events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SubmitAck {
    pub command_id: CommandId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn submit_ack_round_trips_and_shapes() {
        let ack = SubmitAck {
            command_id: CommandId(9),
        };
        assert_eq!(
            serde_json::to_value(ack).unwrap(),
            json!({ "command_id": 9 })
        );
        let back: SubmitAck = serde_json::from_value(serde_json::to_value(ack).unwrap()).unwrap();
        assert_eq!(ack, back);
    }
}
