//! Policy: what a keystroke is on the wire — the measured table from a [`KeyPress`] plus
//! the modes the far end turned on to the bytes a terminal sends for it.
//!
//! A pure function beside the grid rather than in the frontend, and that placement is the
//! whole point of the seam (roadmap 28, spec 28 decision 4). An arrow key is not one byte
//! sequence but two — `ESC [ A` normally and `ESC O A` once the far end has turned on
//! application cursor keys — and only the emulator knows which mode it is in. A frontend
//! that hard-coded one of them works at a bare `cmd` prompt and sends something else
//! entirely into a far end that asked for the other, and the failure is silent: the far end
//! simply does a different thing and says nothing about it.
//!
//! **Every row here was measured on 2026-09-02**, against `bash` under WSL, `pwsh` 7.6.5
//! and `cmd.exe`, with the rig in `crates/acter-transports/examples/capture.rs`: type a
//! line, move with each candidate spelling, type a marker character, and read where the
//! marker landed. Nothing is here that was not measured, which is why the function keys
//! are absent — a guessed spelling is worse than a missing one, because the far end
//! answers it and nobody hears why.

use crate::{Key, KeyPress, TerminalModes};

/// What a terminal sends for the Escape key, and the prefix `Alt` puts in front of
/// everything else.
const ESC: u8 = 0x1b;

/// The bytes this keystroke is, for a far end in these modes.
///
/// Total: every keystroke has an answer, because a key with no spelling still has to be
/// something rather than a panic. A character with no modifier is its own UTF-8, which is
/// the ordinary case and the one that needs no table at all.
///
/// # The table
///
/// - **Backspace is `0x7f`, and `0x08` is a defect.** In `readline` both delete one
///   character, which is how a wrong answer here survives casual testing. In **PSReadLine
///   and in `cmd.exe`, `0x08` deletes the previous *word*** — measured, a line went from
///   `BAhello worldECD` to `BAhello CD` on one press — while `0x7f` deletes one character
///   on all three far ends. This is the silent-garbage failure roadmap 28 predicted for the
///   arrows and did not find there. `0x08` is what `Ctrl+Backspace` becomes, which is the
///   only place it belongs.
/// - **Home is `ESC[H` and End is `ESC[F`.** Both far ends took the `ESC[1~` and `ESC[4~`
///   spellings as well; one of the two had to be chosen, and these are the ones a terminal
///   sends when the far end has not asked for anything else.
/// - **Delete is `ESC[3~`** on all three, and stays that spelling whatever the cursor-key
///   mode is: it is a tilde-terminated key rather than a cursor key.
/// - **The arrows are `ESC[A`, `ESC[B`, `ESC[C` and `ESC[D`**, or the `ESC O` forms — which
///   Home and End share — once the far end has turned on application cursor keys.
/// - **`Ctrl` plus a letter is that letter's control byte**, which is how `Ctrl+C`,
///   `Ctrl+D` and `Ctrl+U` reach the far end in far-end-line mode with no special case at
///   all: `0x03`, `0x04`, `0x15`. What the far end does with `Ctrl+U` is the far end's
///   business — `readline` and PSReadLine clear the line, `cmd.exe` inserts a literal `^U`
///   into it — which is why "the row a key emptied" is something Acter reports and never
///   promises.
/// - **`Alt` puts `ESC` in front of whatever the key already was**, which is how a terminal
///   has spelled a meta key since before there was a meta key to spell.
pub fn key_bytes(press: &KeyPress, modes: TerminalModes) -> Vec<u8> {
    let mut bytes = unmodified(press, modes);
    if press.alt {
        bytes.insert(0, ESC);
    }
    bytes
}

/// Everything but the `Alt` prefix.
fn unmodified(press: &KeyPress, modes: TerminalModes) -> Vec<u8> {
    match press.key {
        Key::Char(character) => character_bytes(character, press.ctrl),
        // The six keys application cursor mode respells. `Shift` is deliberately not
        // consulted: the shifted forms are a modifyOtherKeys spelling nobody measured, and
        // a far end that never asked for them would read the parameters as something else.
        Key::Up => cursor_key(b'A', modes),
        Key::Down => cursor_key(b'B', modes),
        Key::Right => cursor_key(b'C', modes),
        Key::Left => cursor_key(b'D', modes),
        Key::Home => cursor_key(b'H', modes),
        Key::End => cursor_key(b'F', modes),
        Key::Tab => vec![0x09],
        // A carriage return and never a line feed. A real shell on a pseudoconsole echoes
        // a line feed and goes on waiting for the Enter that never came, so with `\n` here
        // every line would look accepted and silently do nothing (spec B4).
        Key::Enter => vec![b'\r'],
        // The one line of this file with a measurement behind it that changes what a far
        // end does to the user's text. See the table above.
        Key::Backspace => vec![if press.ctrl { 0x08 } else { 0x7f }],
        Key::Delete => vec![ESC, b'[', b'3', b'~'],
        Key::Escape => vec![ESC],
    }
}

/// `ESC [ x` normally, `ESC O x` once the far end has turned on application cursor keys.
fn cursor_key(final_byte: u8, modes: TerminalModes) -> Vec<u8> {
    let introducer = if modes.application_cursor_keys {
        b'O'
    } else {
        b'['
    };
    vec![ESC, introducer, final_byte]
}

/// A character key: its own UTF-8, or the control byte `Ctrl` makes of a letter.
///
/// A `Ctrl` held over anything that is not an ASCII letter falls through to the character
/// itself. The control bytes for the punctuation range — `Ctrl+@`, `Ctrl+[` and the rest —
/// are real, and they are not here because nobody has measured which of them a browser even
/// reports, and a keystroke that reaches the far end as the wrong byte is exactly what this
/// module exists to prevent.
fn character_bytes(character: char, ctrl: bool) -> Vec<u8> {
    if ctrl && character.is_ascii_alphabetic() {
        // `a` is 0x61 and `Ctrl+A` is 0x01; the same arithmetic on the upper-case letter is
        // what a terminal has always done.
        return vec![character.to_ascii_uppercase() as u8 - 0x40];
    }
    let mut buffer = [0u8; 4];
    character.encode_utf8(&mut buffer).as_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key) -> KeyPress {
        KeyPress {
            key,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    fn plain() -> TerminalModes {
        TerminalModes {
            application_cursor_keys: false,
            bracketed_paste: false,
        }
    }

    fn application() -> TerminalModes {
        TerminalModes {
            application_cursor_keys: true,
            bracketed_paste: false,
        }
    }

    /// **The single highest-risk row of the entry, pinned with its reason.**
    ///
    /// `0x08` and `0x7f` both delete one character in `readline`, so a wrong answer here
    /// passes every test anybody would run against `bash`. Measured 2026-09-02 against
    /// PSReadLine and `cmd.exe`, `0x08` deletes the previous **word**: one press took
    /// `BAhello worldECD` to `BAhello CD`. A user who cannot see the line would have no way
    /// to know their command had lost a word.
    #[test]
    fn backspace_is_delete_and_never_the_word_eating_byte() {
        assert_eq!(
            key_bytes(&press(Key::Backspace), plain()),
            vec![0x7f],
            "0x08 deletes the previous word in PSReadLine and cmd.exe"
        );
        assert_eq!(
            key_bytes(&press(Key::Backspace), application()),
            vec![0x7f],
            "and the cursor-key mode has nothing to do with it"
        );
    }

    /// The other half of the same measurement: `0x08` is a real key, and it is this one.
    #[test]
    fn ctrl_backspace_is_the_byte_that_eats_a_word() {
        let press = KeyPress {
            key: Key::Backspace,
            ctrl: true,
            shift: false,
            alt: false,
        };
        assert_eq!(key_bytes(&press, plain()), vec![0x08]);
    }

    /// The table, stated as a table, for a far end that has asked for nothing.
    #[test]
    fn every_named_key_is_its_measured_spelling() {
        let rows: [(Key, &[u8]); 11] = [
            (Key::Up, b"\x1b[A"),
            (Key::Down, b"\x1b[B"),
            (Key::Right, b"\x1b[C"),
            (Key::Left, b"\x1b[D"),
            (Key::Home, b"\x1b[H"),
            (Key::End, b"\x1b[F"),
            (Key::Delete, b"\x1b[3~"),
            (Key::Tab, b"\t"),
            (Key::Enter, b"\r"),
            (Key::Backspace, b"\x7f"),
            (Key::Escape, b"\x1b"),
        ];
        for (key, expected) in rows {
            assert_eq!(key_bytes(&press(key), plain()), expected, "for {key:?}");
        }
    }

    /// And the same table for a far end that has turned application cursor keys on, which
    /// is what `readline`-driven shells and most full-screen programs do the moment they
    /// take the keyboard.
    #[test]
    fn application_cursor_keys_respell_the_six_and_nothing_else() {
        let rows: [(Key, &[u8]); 11] = [
            (Key::Up, b"\x1bOA"),
            (Key::Down, b"\x1bOB"),
            (Key::Right, b"\x1bOC"),
            (Key::Left, b"\x1bOD"),
            (Key::Home, b"\x1bOH"),
            (Key::End, b"\x1bOF"),
            // Not a cursor key: a tilde-terminated one, and unchanged by the mode.
            (Key::Delete, b"\x1b[3~"),
            (Key::Tab, b"\t"),
            (Key::Enter, b"\r"),
            (Key::Backspace, b"\x7f"),
            (Key::Escape, b"\x1b"),
        ];
        for (key, expected) in rows {
            assert_eq!(
                key_bytes(&press(key), application()),
                expected,
                "for {key:?}"
            );
        }
    }

    /// `Ctrl` plus a letter is the control byte, which is how the three keys this mode has
    /// to carry reach the far end with no special case anywhere: interrupt, end of input,
    /// and discard the line.
    #[test]
    fn ctrl_and_a_letter_is_that_letters_control_byte() {
        for (letter, expected) in [('c', 0x03), ('d', 0x04), ('u', 0x15), ('a', 0x01)] {
            let press = KeyPress {
                key: Key::Char(letter),
                ctrl: true,
                shift: false,
                alt: false,
            };
            assert_eq!(key_bytes(&press, plain()), vec![expected], "for {letter}");
        }
    }

    /// The case letters are typed in does not change which control byte they are, because a
    /// terminal has never distinguished them.
    #[test]
    fn a_capital_letter_is_the_same_control_byte() {
        let upper = KeyPress {
            key: Key::Char('C'),
            ctrl: true,
            shift: true,
            alt: false,
        };
        assert_eq!(key_bytes(&upper, plain()), vec![0x03]);
    }

    /// An ordinary character is its own UTF-8 and nothing else happens to it, which is the
    /// commonest keystroke there is.
    #[test]
    fn a_character_is_its_own_bytes() {
        assert_eq!(key_bytes(&press(Key::Char('a')), plain()), b"a");
        assert_eq!(key_bytes(&press(Key::Char('ç')), plain()), "ç".as_bytes());
        assert_eq!(key_bytes(&press(Key::Char(' ')), plain()), b" ");
    }

    /// `Ctrl` over something that is not a letter has no measured control byte, so the
    /// character goes as itself rather than as a guess.
    #[test]
    fn ctrl_over_a_non_letter_sends_the_character() {
        let press = KeyPress {
            key: Key::Char('1'),
            ctrl: true,
            shift: false,
            alt: false,
        };
        assert_eq!(key_bytes(&press, plain()), b"1");
    }

    /// `Alt` is an `ESC` in front of whatever the key already was — for a character, for a
    /// named key, and for a control byte alike.
    #[test]
    fn alt_prefixes_escape_onto_whatever_the_key_was() {
        let rows: [(KeyPress, &[u8]); 3] = [
            (
                KeyPress {
                    key: Key::Char('b'),
                    ctrl: false,
                    shift: false,
                    alt: true,
                },
                b"\x1bb",
            ),
            (
                KeyPress {
                    key: Key::Left,
                    ctrl: false,
                    shift: false,
                    alt: true,
                },
                b"\x1b\x1b[D",
            ),
            (
                KeyPress {
                    key: Key::Char('d'),
                    ctrl: true,
                    shift: false,
                    alt: true,
                },
                b"\x1b\x04",
            ),
        ];
        for (press, expected) in rows {
            assert_eq!(key_bytes(&press, plain()), expected, "for {press:?}");
        }
    }

    /// Pure in the sense that matters: the same keystroke in the same modes is always the
    /// same bytes, so a far end cannot be sent two different things for one key.
    #[test]
    fn the_same_keystroke_is_always_the_same_bytes() {
        let press = press(Key::Up);
        assert_eq!(key_bytes(&press, plain()), key_bytes(&press, plain()));
    }
}
