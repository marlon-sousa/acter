//! Policy: the keybinding table — a pure function from a keystroke to what it means to
//! the session, and nothing else.
//!
//! A decision table is exactly what a policy is: deterministic in, deterministic out, no
//! clock, no ports, no state. Returning `None` for a key nothing is bound to is the
//! honest answer rather than a failure — most keys mean nothing to a session, and the
//! frontend has to be able to say so (spec B6, decision 6).
//!
//! **Configurability is deliberately deferred, and the seam is the deliverable.**
//! DESIGN Decided that all keybindings are configurable and global, and this entry does
//! not build that: profiles and the configuration screen are post-convergence, and
//! inventing a settings store here would be the largest thing in the PR and the least
//! tested. What matters now is that the table lives *behind the port*, on the backend
//! side, where the profile machinery will be — making it configurable is then replacing
//! a constant with a loaded value, with no frontend or protocol change involved.

use crate::{Key, KeyPress, SessionIntent};

/// What this keystroke means to the session, if anything.
///
/// The whole table, and it fits in one match: DESIGN's keystroke map layer 2 gives
/// `Ctrl+C` without a selection to the running command, and the selection half of that
/// sentence never reaches here — a `Ctrl+C` *with* a selection is a copy the edit field
/// consumed locally and never reported.
///
/// **`Ctrl+D` is layer 2 rather than a new default binding** (spec B5.2): DESIGN lists
/// default bindings only for layer 1, the `Ctrl+Shift` combinations that are Acter's own,
/// and says of layer 2 that contextual keys keep their native meaning per focus. In an
/// edit field standing in for a terminal's command line, `Ctrl+D`'s native meaning is end
/// of input. What that costs in bytes is the shell's answer, not this table's.
pub fn intent_for(press: &KeyPress) -> Option<SessionIntent> {
    match press {
        KeyPress {
            key: Key::Char('c'),
            ctrl: true,
            shift: false,
            alt: false,
        } => Some(SessionIntent::Interrupt),
        KeyPress {
            key: Key::Char('d'),
            ctrl: true,
            shift: false,
            alt: false,
        } => Some(SessionIntent::Eof),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(ch: char, ctrl: bool, shift: bool, alt: bool) -> KeyPress {
        KeyPress {
            key: Key::Char(ch),
            ctrl,
            shift,
            alt,
        }
    }

    /// The table, stated as a table. Every row that is not a binding is `None`,
    /// which is the half worth pinning: a session that guessed at unbound keys would
    /// act on keystrokes the user aimed somewhere else.
    #[test]
    fn the_table_binds_ctrl_c_and_ctrl_d_and_nothing_else() {
        let rows = [
            (
                press('c', true, false, false),
                Some(SessionIntent::Interrupt),
            ),
            (press('d', true, false, false), Some(SessionIntent::Eof)),
            // The plain letters are text the edit field owns.
            (press('c', false, false, false), None),
            (press('d', false, false, false), None),
            // A different modifier combination is a different keystroke, and DESIGN's
            // layer 1 (Ctrl+Shift+letter) is Acter's own and never arrives here at all.
            (press('c', true, true, false), None),
            (press('c', true, false, true), None),
            (press('d', true, true, false), None),
            (press('d', true, false, true), None),
            (press('x', false, false, false), None),
        ];
        for (press, expected) in rows {
            assert_eq!(intent_for(&press), expected, "for {press:?}");
        }
    }

    /// The two bindings are two intents, which is the whole reason the table is a
    /// function rather than a boolean: a session that collapsed them would stop a
    /// running command when the user asked to end the session.
    #[test]
    fn interrupting_and_ending_are_not_the_same_keystroke() {
        assert_ne!(
            intent_for(&press('c', true, false, false)),
            intent_for(&press('d', true, false, false))
        );
    }

    /// Pure in the sense that matters: the same keystroke always answers the same thing.
    #[test]
    fn the_same_keystroke_always_means_the_same_thing() {
        let press = press('c', true, false, false);
        assert_eq!(intent_for(&press), intent_for(&press));
    }
}
