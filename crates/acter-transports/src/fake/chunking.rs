//! Policy: how a delivery is cut into reads.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Chunking {
    #[default]
    Whole,
    /// Zero is read as one.
    Bytes(usize),
}

impl Chunking {
    pub(crate) fn cut(self, bytes: &[u8]) -> Vec<&[u8]> {
        if bytes.is_empty() {
            return Vec::new();
        }
        match self {
            Self::Whole => vec![bytes],
            Self::Bytes(size) => bytes.chunks(size.max(1)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD: &[u8] = b"\x1b]133;C\x07line 1\r\nline 2\r\n\x1b[?1049h\x1b[H\x1b[2J done";

    #[test]
    fn whole_is_the_identity() {
        assert_eq!(Chunking::Whole.cut(PAYLOAD), vec![PAYLOAD]);
    }

    #[test]
    fn cutting_never_loses_a_byte_or_moves_one() {
        for chunking in [
            Chunking::Whole,
            Chunking::Bytes(1),
            Chunking::Bytes(2),
            Chunking::Bytes(7),
            Chunking::Bytes(PAYLOAD.len()),
            Chunking::Bytes(PAYLOAD.len() + 1),
        ] {
            let reads = chunking.cut(PAYLOAD);
            assert_eq!(
                reads.concat(),
                PAYLOAD,
                "{chunking:?} must cut, never drop or reorder"
            );
            assert!(
                reads.iter().all(|read| !read.is_empty()),
                "{chunking:?} produced an empty read"
            );
        }
    }

    #[test]
    fn one_byte_at_a_time_is_one_read_per_byte() {
        let reads = Chunking::Bytes(1).cut(b"abc");
        assert_eq!(reads, vec![b"a".as_slice(), b"b", b"c"]);
    }

    #[test]
    fn nothing_to_say_is_no_read_at_all() {
        assert!(Chunking::Whole.cut(b"").is_empty());
        assert!(Chunking::Bytes(1).cut(b"").is_empty());
    }

    #[test]
    fn a_zero_sized_cut_is_read_as_one_byte() {
        assert_eq!(Chunking::Bytes(0).cut(b"ab"), Chunking::Bytes(1).cut(b"ab"));
    }
}
