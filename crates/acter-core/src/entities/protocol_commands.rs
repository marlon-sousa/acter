//! Entity/value: the payloads of the frontend-to-backend command (invoke) surface —
//! what an invoke carries in, and what it answers with.
//!
//! An invoke never waits on the shell (ARCHITECTURE, IPC rules): `submit_command`
//! returns immediately with the correlation id every later event carries. Phase-1
//! invoke *arguments* were all primitives (`session_id`, `line`, `cols`, `rows`) passed
//! straight to routers in A3; [`KeyPress`] is the first that is not, because a keystroke
//! is four facts that only mean anything together. Completion (A4) and session-snapshot
//! (A3) payloads are defined with the domains that build them.
//!
//! **The frontend reports the key, never the meaning** (spec B6, decision 4). What
//! `Ctrl+C` *does* is a binding, bindings are configuration, and configuration is the
//! backend's: the map from a keystroke to a
//! [`SessionIntent`](crate::SessionIntent) is `policies::keybindings`, behind this
//! seam. Only the keys the frontend did not claim for itself ever arrive here — layer 1
//! is Acter's own commands and `Ctrl+C` *with* a selection is a copy, both consumed
//! locally — so this is a short list, not every keypress crossing the IPC boundary.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::CommandId;

/// The immediate return of `submit_command`: the id correlating this submission with
/// its `CommandStarted` / `Output` / `CommandFinished` events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SubmitAck {
    pub command_id: CommandId,
}

/// One keystroke the frontend did not consume, described rather than interpreted.
///
/// Modifiers are flags rather than a set because a keystroke has exactly these three
/// and a listener never asks "which modifiers", only "was Ctrl held".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KeyPress {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Which key, with exactly the one variant something presses today.
///
/// Named variants (Tab, Escape, the arrows, the function keys) arrive when an entry
/// needs them — Tab with A4's completion, the rest with phase 2's pass-through key —
/// and not before. Shipping the whole keyboard now would be a dozen variants with no
/// consumer, which is the shape B1 refused to create and B3.5 decision 10 restated
/// (spec B6, decision 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Key {
    /// A character key, as the frontend read it off the keyboard event.
    Char(char),
}

/// What became of a keystroke: the two questions the frontend cannot answer itself.
///
/// A key nothing is bound to and a bound key that found nothing running are different
/// things to say to a listener, so they are different answers here. Which words each
/// one becomes is the frontend's (A3.2); this only reports what happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum KeyAck {
    /// No binding for this keystroke. Nothing was attempted.
    Unbound,
    /// Bound, and acted on: the intent reached the session.
    Applied,
    /// Bound, but there was no running command to act on.
    ///
    /// The honest answer to A3.1 decision 6's "nothing to stop", which the typed `stop`
    /// had no way to give.
    NothingToActOn,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A keystroke crosses the wire, so its shape is pinned like every other IPC type.
    #[test]
    fn a_key_press_round_trips_and_shapes() {
        let press = KeyPress {
            key: Key::Char('c'),
            ctrl: true,
            shift: false,
            alt: false,
        };
        assert_eq!(
            serde_json::to_value(&press).unwrap(),
            json!({ "key": { "Char": "c" }, "ctrl": true, "shift": false, "alt": false })
        );
        let back: KeyPress = serde_json::from_value(serde_json::to_value(&press).unwrap()).unwrap();
        assert_eq!(press, back);
    }

    /// Every answer, so a new one cannot be added without deciding what it means.
    #[test]
    fn every_key_ack_round_trips_as_a_bare_name() {
        for ack in [KeyAck::Unbound, KeyAck::Applied, KeyAck::NothingToActOn] {
            let json = serde_json::to_value(ack).unwrap();
            assert!(json.is_string(), "a unit answer is a bare name: {json}");
            let back: KeyAck = serde_json::from_value(json).unwrap();
            assert_eq!(ack, back);
        }
    }

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
