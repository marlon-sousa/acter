//! Policy: what a keystroke is on the wire — the measured table from a [`KeyPress`] plus
//! the modes the far end turned on to the bytes a terminal sends for it.
//!
//! Every row below was measured against `bash` under WSL, `pwsh` 7.6.5 and `cmd.exe`,
//! using the rig in `crates/acter-transports/examples/capture.rs`. Function keys
//! are absent because they were not measured.

use crate::{Key, KeyPress, TerminalModes};

const ESC: u8 = 0x1b;

/// # The table
///
/// - Backspace is `0x7f`; `0x08` is `Ctrl+Backspace`: `0x08` deletes the previous word in
///   PSReadLine and `cmd.exe` (`BAhello worldECD` → `BAhello CD`), while `0x7f` deletes
///   one character on all three far ends.
/// - Home is `ESC[H`; End is `ESC[F`. `ESC[1~` and `ESC[4~` are also accepted by all three
///   far ends but are not sent.
/// - Delete is `ESC[3~` on all three, unaffected by the cursor-key mode.
/// - Arrows are `ESC[A/B/C/D`, switching to `ESC O A/B/C/D` (shared with Home and End)
///   once application cursor keys are on.
/// - `Ctrl` plus a letter is that letter's control byte (`Ctrl+C` is `0x03`, `Ctrl+D`
///   is `0x04`, `Ctrl+U` is `0x15`). What `Ctrl+U` does is the far end's business:
///   `readline` and PSReadLine clear the line, `cmd.exe` inserts a literal `^U`.
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
        // `Shift` is not consulted: the shifted spellings are an unmeasured modifyOtherKeys
        // form.
        Key::Up => cursor_key(b'A', modes),
        Key::Down => cursor_key(b'B', modes),
        Key::Right => cursor_key(b'C', modes),
        Key::Left => cursor_key(b'D', modes),
        Key::Home => cursor_key(b'H', modes),
        Key::End => cursor_key(b'F', modes),
        Key::Tab => vec![0x09],
        // Carriage return, never line feed: a shell on a pseudoconsole waits for the `\r`
        // that never came and the line looks silently accepted.
        Key::Enter => vec![b'\r'],
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

/// Ctrl held over anything but an ASCII letter falls through to the character itself; the
/// control bytes for punctuation (`Ctrl+@`, `Ctrl+[`, ...) are not implemented because they
/// have not been measured.
fn character_bytes(character: char, ctrl: bool) -> Vec<u8> {
    if ctrl && character.is_ascii_alphabetic() {
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

    #[test]
    fn application_cursor_keys_respell_the_six_and_nothing_else() {
        let rows: [(Key, &[u8]); 11] = [
            (Key::Up, b"\x1bOA"),
            (Key::Down, b"\x1bOB"),
            (Key::Right, b"\x1bOC"),
            (Key::Left, b"\x1bOD"),
            (Key::Home, b"\x1bOH"),
            (Key::End, b"\x1bOF"),
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

    #[test]
    fn a_character_is_its_own_bytes() {
        assert_eq!(key_bytes(&press(Key::Char('a')), plain()), b"a");
        assert_eq!(key_bytes(&press(Key::Char('ç')), plain()), "ç".as_bytes());
        assert_eq!(key_bytes(&press(Key::Char(' ')), plain()), b" ");
    }

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

    #[test]
    fn the_same_keystroke_is_always_the_same_bytes() {
        let press = press(Key::Up);
        assert_eq!(key_bytes(&press, plain()), key_bytes(&press, plain()));
    }
}
