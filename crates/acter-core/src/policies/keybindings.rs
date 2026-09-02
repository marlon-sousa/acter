//! Policy: the keybinding table — a pure function from a keystroke and who owns the line
//! to what it means to the session, and nothing else.
//!
//! A decision table is exactly what a policy is: deterministic in, deterministic out, no
//! clock, no ports, no state. Returning [`Binding::Unbound`] for a key nothing is bound to
//! is the honest answer rather than a failure — most keys mean nothing to a session, and
//! the frontend has to be able to say so (spec B6, decision 6).
//!
//! **Configurability is deliberately deferred, and the seam is the deliverable.**
//! DESIGN Decided that all keybindings are configurable and global, and this entry does
//! not build that: profiles and the configuration screen are post-convergence, and
//! inventing a settings store here would be the largest thing in the PR and the least
//! tested. What matters now is that the table lives *behind the port*, on the backend
//! side, where the profile machinery will be — making it configurable is then replacing
//! a constant with a loaded value, with no frontend or protocol change involved.
//!
//! **It takes the line's owner since 28**, rather than a second table growing beside it.
//! Far-end-line mode is not a set of keys that mean something else; it is a state in which
//! *no* key means anything to Acter, because every one of them is the far end's. Expressing
//! that as one more argument to the one table is what keeps the frontend from having to
//! know it at all: it still reports the key, and the answer to "what happens to it" is
//! still on this side of the wire (spec 28, decision 4).

use crate::{Key, KeyPress, LineOwner, SessionIntent};

/// What becomes of one keystroke.
///
/// Three answers rather than two, because far-end-line mode adds a destination rather than
/// a meaning: a key on its way to the far end has no intent at all — it is bytes, and which
/// bytes is [`key_bytes`](crate::key_bytes)'s question, asked where the far end's modes are
/// known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// Acter acts on it, and this is what it means.
    Intent(SessionIntent),
    /// The far end gets it, as the bytes a terminal sends for that key.
    ToFarEnd,
    /// Nothing is bound to it. Nothing was attempted.
    Unbound,
}

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
///
/// **With the far end owning the line the table is empty, and that is the decision rather
/// than an omission** (spec 28, decision 4). `Ctrl+C`, `Ctrl+D` and `Ctrl+U` all reach the
/// far end there as their own control bytes — `0x03`, `0x04`, `0x15` — which is not the
/// same thing as Acter's interrupt and Acter's end-of-input, and the difference is exactly
/// what makes the mode worth having: inside an `ssh`, today's `Eof` sends the *local*
/// PowerShell's `exit` into the wrong shell.
pub fn binding_for(press: &KeyPress, owner: LineOwner) -> Binding {
    if owner == LineOwner::FarEnd {
        return Binding::ToFarEnd;
    }
    match press {
        KeyPress {
            key: Key::Char('c'),
            ctrl: true,
            shift: false,
            alt: false,
        } => Binding::Intent(SessionIntent::Interrupt),
        KeyPress {
            key: Key::Char('d'),
            ctrl: true,
            shift: false,
            alt: false,
        } => Binding::Intent(SessionIntent::Eof),
        _ => Binding::Unbound,
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

    /// The table, stated as a table. Every row that is not a binding is `Unbound`,
    /// which is the half worth pinning: a session that guessed at unbound keys would
    /// act on keystrokes the user aimed somewhere else.
    #[test]
    fn the_table_binds_ctrl_c_and_ctrl_d_and_nothing_else() {
        let rows = [
            (
                press('c', true, false, false),
                Binding::Intent(SessionIntent::Interrupt),
            ),
            (
                press('d', true, false, false),
                Binding::Intent(SessionIntent::Eof),
            ),
            // The plain letters are text the edit field owns.
            (press('c', false, false, false), Binding::Unbound),
            (press('d', false, false, false), Binding::Unbound),
            // A different modifier combination is a different keystroke, and DESIGN's
            // layer 1 (Ctrl+Shift+letter) is Acter's own and never arrives here at all.
            (press('c', true, true, false), Binding::Unbound),
            (press('c', true, false, true), Binding::Unbound),
            (press('d', true, true, false), Binding::Unbound),
            (press('d', true, false, true), Binding::Unbound),
            (press('x', false, false, false), Binding::Unbound),
        ];
        for (press, expected) in rows {
            assert_eq!(
                binding_for(&press, LineOwner::Local),
                expected,
                "for {press:?}"
            );
        }
    }

    /// The two bindings are two intents, which is the whole reason the table is a
    /// function rather than a boolean: a session that collapsed them would stop a
    /// running command when the user asked to end the session.
    #[test]
    fn interrupting_and_ending_are_not_the_same_keystroke() {
        assert_ne!(
            binding_for(&press('c', true, false, false), LineOwner::Local),
            binding_for(&press('d', true, false, false), LineOwner::Local)
        );
    }

    /// **With the far end owning the line, every key is the far end's** — the two Acter
    /// binds locally included. That is the mode: `Ctrl+C` is `0x03` on the wire rather than
    /// an interrupt Acter asked the transport for, and `Ctrl+D` is `0x04` aimed at whatever
    /// is actually reading rather than at the shell Acter spawned.
    #[test]
    fn the_far_end_gets_every_key_including_the_two_acter_binds() {
        for press in [
            press('c', true, false, false),
            press('d', true, false, false),
            press('u', true, false, false),
            press('a', false, false, false),
            KeyPress {
                key: Key::Up,
                ctrl: false,
                shift: false,
                alt: false,
            },
            KeyPress {
                key: Key::Tab,
                ctrl: false,
                shift: false,
                alt: false,
            },
        ] {
            assert_eq!(
                binding_for(&press, LineOwner::FarEnd),
                Binding::ToFarEnd,
                "for {press:?}"
            );
        }
    }

    /// A named key means nothing to Acter while Acter owns the line: the arrows there are
    /// the edit field's own history and the field's own caret, and neither is a session
    /// intent.
    #[test]
    fn a_named_key_is_unbound_while_acter_owns_the_line() {
        for key in [Key::Up, Key::Tab, Key::Backspace, Key::Escape] {
            let press = KeyPress {
                key,
                ctrl: false,
                shift: false,
                alt: false,
            };
            assert_eq!(binding_for(&press, LineOwner::Local), Binding::Unbound);
        }
    }

    /// Pure in the sense that matters: the same keystroke always answers the same thing.
    #[test]
    fn the_same_keystroke_always_means_the_same_thing() {
        let press = press('c', true, false, false);
        assert_eq!(
            binding_for(&press, LineOwner::Local),
            binding_for(&press, LineOwner::Local)
        );
    }
}
