//! Policy: the keybinding table — a pure function from a keystroke and who owns the line
//! to what it means to the session, and nothing else.

use crate::{Key, KeyPress, LineOwner, SessionIntent};

/// What becomes of one keystroke.
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
/// Ctrl+C with an active selection is consumed by the edit field as a copy and never
/// reaches here; only the without-selection case does.
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
            (press('c', false, false, false), Binding::Unbound),
            (press('d', false, false, false), Binding::Unbound),
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

    #[test]
    fn interrupting_and_ending_are_not_the_same_keystroke() {
        assert_ne!(
            binding_for(&press('c', true, false, false), LineOwner::Local),
            binding_for(&press('d', true, false, false), LineOwner::Local)
        );
    }

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

    #[test]
    fn the_same_keystroke_always_means_the_same_thing() {
        let press = press('c', true, false, false);
        assert_eq!(
            binding_for(&press, LineOwner::Local),
            binding_for(&press, LineOwner::Local)
        );
    }
}
