//! Policy: how a delivery is cut into reads.
//!
//! **A removal of expressiveness, on purpose** (spec B3.6, decision 3). Read boundaries
//! used to be authored: a transcript said "these two steps, no delay between them", and
//! DESIGN's "a marker split across two reads" was therefore proven for exactly one
//! hand-written marker. Splitting is a property of how a read happens to land, not of
//! what the far end said, so it moved here — and became a dimension every fixture is
//! replayed under instead of a fixture of its own.
//!
//! **It cuts; it never drops or reorders.** A pipe that loses bytes is a different
//! failure model, and inventing one before anything tests against it would be guessing
//! at what a broken transport does.
//!
//! Pure: no clock, no randomness, no I/O. Deterministic in, deterministic out.

/// How the pipe cuts each delivery into reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Chunking {
    /// One delivery, one read. The default, and what every transcript means.
    #[default]
    Whole,
    /// Reads of at most this many bytes. `Bytes(1)` is the adversarial case: every
    /// marker, every escape sequence and every line arrives one byte at a time, which is
    /// what B2's cardinal property (text is never lost) and B3's marker recognition are
    /// worth asserting against.
    ///
    /// Zero is read as one, because a read of no bytes is not a read.
    Bytes(usize),
}

impl Chunking {
    /// The reads one delivery becomes.
    ///
    /// Empty in, empty out: a delivery with nothing to say produces no read at all,
    /// rather than a zero-byte one that a reader would have to know to ignore. That is
    /// how a decorator can empty a delivery — [`Unmarked`](super::Unmarked) removing a
    /// marker that was the whole payload — without the pipe emitting anything for it.
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

    /// The payload of the longest single delivery any fixture has, so the properties
    /// below are asserted over something with real escape sequences in it rather than a
    /// tidy string.
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

    /// A delivery a decorator emptied is not a read: see [`Chunking::cut`].
    #[test]
    fn nothing_to_say_is_no_read_at_all() {
        assert!(Chunking::Whole.cut(b"").is_empty());
        assert!(Chunking::Bytes(1).cut(b"").is_empty());
    }

    /// Zero would otherwise mean an endless sequence of empty reads, which is the one
    /// answer that is certainly wrong.
    #[test]
    fn a_zero_sized_cut_is_read_as_one_byte() {
        assert_eq!(Chunking::Bytes(0).cut(b"ab"), Chunking::Bytes(1).cut(b"ab"));
    }
}
