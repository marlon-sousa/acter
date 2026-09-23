//! Policy: reading a list of distribution names out of what `wsl.exe -l -q` wrote.
//!
//! `wsl.exe -l -q` in WSL 2.5.7.0 writes UTF-16LE with a CRLF after every name and no
//! byte-order mark.

pub(crate) fn distributions(bytes: &[u8]) -> Vec<String> {
    decode_utf16le(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// `wsl.exe` in WSL 2.5.7.0 writes its refusals in UTF-16LE to standard output with standard
/// error empty, as `There is no distribution with the supplied name.` with exit code 127.
pub(crate) fn decode_utf16le(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
    let (pairs, _leftover) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().copied().map(u16::from_le_bytes).collect();
    char::decode_utf16(units)
        .map(|character| character.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured verbatim from `wsl.exe -l -q`.
    const CAPTURED: &[u8] = &[
        0x55, 0x00, 0x62, 0x00, 0x75, 0x00, 0x6e, 0x00, 0x74, 0x00, 0x75, 0x00, 0x0d, 0x00, 0x0a,
        0x00, 0x64, 0x00, 0x6f, 0x00, 0x63, 0x00, 0x6b, 0x00, 0x65, 0x00, 0x72, 0x00, 0x2d, 0x00,
        0x64, 0x00, 0x65, 0x00, 0x73, 0x00, 0x6b, 0x00, 0x74, 0x00, 0x6f, 0x00, 0x70, 0x00, 0x0d,
        0x00, 0x0a, 0x00, 0x44, 0x00, 0x65, 0x00, 0x62, 0x00, 0x69, 0x00, 0x61, 0x00, 0x6e, 0x00,
        0x0d, 0x00, 0x0a, 0x00,
    ];

    #[test]
    fn the_captured_output_of_a_real_wsl_reads_as_three_distribution_names() {
        assert_eq!(
            distributions(CAPTURED),
            ["Ubuntu", "docker-desktop", "Debian"]
        );
    }

    #[test]
    fn reading_the_same_bytes_as_utf8_would_not_have_given_a_usable_name() {
        let naive = String::from_utf8_lossy(CAPTURED);

        assert!(
            naive.contains('\0'),
            "which is what makes this decode the substance of the adapter"
        );
        assert_ne!(naive.lines().next(), Some("Ubuntu"));
    }

    #[test]
    fn a_service_distribution_is_offered_rather_than_hidden() {
        assert!(
            distributions(CAPTURED).contains(&"docker-desktop".to_owned()),
            "Acter does not decide which of a user's distributions is a real one"
        );
    }

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

    #[test]
    fn no_name_keeps_the_carriage_return_it_arrived_with() {
        for name in distributions(CAPTURED) {
            assert!(
                !name.contains('\r'),
                "{name:?} still carries its terminator"
            );
        }
    }

    #[test]
    fn an_empty_capture_names_nothing_rather_than_naming_an_empty_string() {
        assert!(distributions(&[]).is_empty());
        assert!(distributions(&[0x0d, 0x00, 0x0a, 0x00]).is_empty());
    }

    #[test]
    fn a_name_outside_the_basic_plane_survives_the_decode() {
        let mut bytes = Vec::new();
        for unit in "Ubuntu-🐧\r\n".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }

        assert_eq!(distributions(&bytes), ["Ubuntu-🐧"]);
    }

    #[test]
    fn a_truncated_capture_still_yields_the_names_that_arrived_whole() {
        let truncated = &CAPTURED[..CAPTURED.len() - 3];

        assert_eq!(
            distributions(truncated),
            ["Ubuntu", "docker-desktop", "Debian"]
        );
    }

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
