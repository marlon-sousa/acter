//! Policy: reading a list of distribution names out of what `wsl.exe -l -q` wrote.
//!
//! **The output is UTF-16LE.** Measured on this machine on 2026-08-24 and again on
//! 2026-08-23 before the spec was written, with `wsl.exe -l -q` redirected to a file and
//! the file dumped byte by byte. The first sixteen bytes were
//! `55 00 62 00 75 00 6e 00 74 00 75 00 0d 00 0a 00` — `U b u n t u CR LF`, one zero byte
//! after each character. Read as UTF-8 that is `U\0b\0u\0n\0t\0u\0`, which is why a naive
//! read gives a name with a null between every letter rather than an obvious error, and
//! why this is the substance of the adapter rather than a detail of it.
//!
//! **No byte-order mark was there.** The spec expected one and told this module to handle
//! it; sixty-four bytes of output on WSL 2.5.7.0 began straight at the first character.
//! One is stripped anyway if it ever appears, because a mark that reached a caller would
//! become an invisible first character of a distribution's name — a name that then fails
//! to match itself, and that a screen reader reads as an unexplained pause.
//!
//! **The line ending is CRLF**, as `0d 00 0a 00` — a carriage return that has to be
//! trimmed off every name, not only off the last one.
//!
//! Pure: bytes in, names out, no process. That is deliberate and is what lets the whole
//! decode be tested against captured bytes rather than against whatever this particular
//! computer happens to have installed (spec B5.3, decision 4).

/// The names in `wsl.exe -l -q`'s output, in the order WSL reported them.
///
/// **Nothing is filtered.** `docker-desktop` and the other service distributions are
/// listed beside the ones a person chose to install, because deciding which of a user's
/// distributions is a "real" one is not knowledge this program has, and the guess would be
/// invisible to exactly the person it was wrong for (spec B5.3, decision 5).
///
/// Lossy rather than fallible: a byte sequence that is not valid UTF-16 yields
/// replacement characters instead of an error, because a machine with one unreadable
/// distribution name should still offer the others. What it must never do is drop a name
/// silently or panic on a name in a script this program has never heard of.
pub(crate) fn distributions(bytes: &[u8]) -> Vec<String> {
    decode_utf16le(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// UTF-16LE bytes as a `String`, byte-order mark removed and unpaired surrogates replaced.
///
/// Also used for the sentence `wsl.exe` writes when it refuses: that text is UTF-16LE too,
/// and it goes to *standard output* rather than to standard error — measured on
/// 2026-08-24, where naming a distribution that does not exist produced
/// `There is no distribution with the supplied name.` on stdout and exit code 127, with
/// standard error empty.
pub(crate) fn decode_utf16le(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
    // `chunks_exact` drops a trailing odd byte rather than inventing a character from it.
    // Truncated output is far likelier than a WSL that emits an odd number of bytes, and
    // half a character is not something anyone can read either way.
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    char::decode_utf16(units)
        .map(|character| character.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact bytes `wsl.exe -l -q` wrote on this machine on 2026-08-24, all
    /// sixty-four of them, spelled out rather than built by a helper: the capture is the
    /// evidence, and a helper that encoded them would be testing itself.
    const CAPTURED: &[u8] = &[
        0x55, 0x00, 0x62, 0x00, 0x75, 0x00, 0x6e, 0x00, 0x74, 0x00, 0x75, 0x00, 0x0d, 0x00, 0x0a,
        0x00, 0x64, 0x00, 0x6f, 0x00, 0x63, 0x00, 0x6b, 0x00, 0x65, 0x00, 0x72, 0x00, 0x2d, 0x00,
        0x64, 0x00, 0x65, 0x00, 0x73, 0x00, 0x6b, 0x00, 0x74, 0x00, 0x6f, 0x00, 0x70, 0x00, 0x0d,
        0x00, 0x0a, 0x00, 0x44, 0x00, 0x65, 0x00, 0x62, 0x00, 0x69, 0x00, 0x61, 0x00, 0x6e, 0x00,
        0x0d, 0x00, 0x0a, 0x00,
    ];

    /// The whole of this module against the real thing: three names, in WSL's order, with
    /// no carriage return clinging to any of them and no empty fourth entry from the
    /// trailing line break.
    #[test]
    fn the_captured_output_of_a_real_wsl_reads_as_three_distribution_names() {
        assert_eq!(
            distributions(CAPTURED),
            ["Ubuntu", "docker-desktop", "Debian"]
        );
    }

    /// The failure a caller would otherwise hit far from here: a name read as UTF-8 keeps
    /// a null byte between every letter, matches nothing, and is spoken as a word with
    /// gaps in it.
    #[test]
    fn reading_the_same_bytes_as_utf8_would_not_have_given_a_usable_name() {
        let naive = String::from_utf8_lossy(CAPTURED);

        assert!(
            naive.contains('\0'),
            "which is what makes this decode the substance of the adapter"
        );
        assert_ne!(naive.lines().next(), Some("Ubuntu"));
    }

    /// `docker-desktop` is listed on purpose, and this test exists to say so: it is the
    /// one name a filter would have been written for, and filtering it is decision 5's
    /// explicit "no".
    #[test]
    fn a_service_distribution_is_offered_rather_than_hidden() {
        assert!(
            distributions(CAPTURED).contains(&"docker-desktop".to_owned()),
            "Acter does not decide which of a user's distributions is a real one"
        );
    }

    /// The spec expected a byte-order mark and this machine emitted none, so both are
    /// handled: one that did arrive would otherwise become an invisible first character
    /// of the first name.
    #[test]
    fn a_byte_order_mark_is_stripped_although_this_machine_emitted_none() {
        let mut with_mark = vec![0xff, 0xfe];
        with_mark.extend_from_slice(CAPTURED);

        assert_eq!(distributions(&with_mark), distributions(CAPTURED));
        assert_eq!(
            CAPTURED[0..2],
            [0x55, 0x00],
            "the capture itself has no mark"
        );
    }

    /// The line ending is CRLF, so a carriage return is on every name and not only the
    /// last. A decode that split on line feeds alone would offer `Ubuntu\r`, which starts
    /// a session in nothing at all.
    #[test]
    fn no_name_keeps_the_carriage_return_it_arrived_with() {
        for name in distributions(CAPTURED) {
            assert!(
                !name.contains('\r'),
                "{name:?} still carries its terminator"
            );
        }
    }

    /// WSL present and empty is a real situation, and it is the one decision 6 needs told
    /// apart from a WSL that is not there at all. Here it is simply no names.
    #[test]
    fn an_empty_capture_names_nothing_rather_than_naming_an_empty_string() {
        assert!(distributions(&[]).is_empty());
        assert!(distributions(&[0x0d, 0x00, 0x0a, 0x00]).is_empty());
    }

    /// A distribution named in a script outside the basic plane: WSL allows one, and a
    /// decode that dropped surrogate pairs would silently rename it.
    #[test]
    fn a_name_outside_the_basic_plane_survives_the_decode() {
        let mut bytes = Vec::new();
        for unit in "Ubuntu-🐧\r\n".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }

        assert_eq!(distributions(&bytes), ["Ubuntu-🐧"]);
    }

    /// Truncated output loses its last half-character rather than the whole list: one
    /// unreadable name must not cost a user the distributions they can still start.
    #[test]
    fn a_truncated_capture_still_yields_the_names_that_arrived_whole() {
        let truncated = &CAPTURED[..CAPTURED.len() - 3];

        assert_eq!(
            distributions(truncated),
            ["Ubuntu", "docker-desktop", "Debian"]
        );
    }

    /// The sentence `wsl.exe` writes when it refuses is UTF-16LE too, and it is read back
    /// so a user hears WSL's own words rather than a null-riddled fragment of them.
    #[test]
    fn a_refusal_from_wsl_decodes_into_a_sentence_a_reader_can_speak() {
        let mut bytes = Vec::new();
        for unit in "There is no distribution with the supplied name.".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }

        assert_eq!(
            decode_utf16le(&bytes),
            "There is no distribution with the supplied name."
        );
    }
}
