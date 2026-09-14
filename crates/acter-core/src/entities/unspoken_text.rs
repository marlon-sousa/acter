//! Entity/value: the text a running command has produced but not yet announced, with
//! the one invariant that keeps a session actor's memory bounded.
//!
//! Once the accumulated span passes the auto-read threshold its verdict is settled as
//! [`ReadMode::TooBig`] forever, since a too-big announcement needs only the line count;
//! from that point the bytes are dropped and only counts are kept. That is what bounds a
//! gapless flood (`yes`, a busy `tail -f`): under a flood no quiescent gap ever occurs, so
//! without this nothing would ever be flushed.
//!
//! One line survives the drop: the final unterminated row, kept beside the counts at the
//! cost of one row of memory. In a session with no shell integration the prompt is that
//! row, and no `PromptDrawn` announces it separately.

use crate::PacingConfig;
use crate::entities::ReadMode;
use crate::policies::{TextSize, measure, verdict};

/// Unannounced text for one command. Line counts stay exact whether or not the text is
/// still held; the character count is exact only while it is, and afterwards is an
/// over-estimate that stays above `max_chars` — which is all the threshold needs, since
/// the verdict is already settled.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct UnspokenText {
    /// `None` once the verdict is settled as too big and the bytes were dropped; this is
    /// why the type cannot derive `Default`, since an empty accumulator holds an empty
    /// string, not nothing.
    text: Option<String>,
    newlines: usize,
    chars: usize,
    ends_with_newline: bool,
    any: bool,
    /// Whatever has arrived since the last line ending. Kept whether or not the bytes are,
    /// because it is one row rather than a span.
    last_line: String,
}

impl Default for UnspokenText {
    fn default() -> Self {
        Self {
            text: Some(String::new()),
            newlines: 0,
            chars: 0,
            ends_with_newline: false,
            any: false,
            last_line: String::new(),
        }
    }
}

impl UnspokenText {
    pub(crate) fn push(&mut self, chunk: &str, config: &PacingConfig) {
        if chunk.is_empty() {
            return;
        }
        self.newlines += chunk.matches('\n').count();
        self.ends_with_newline = chunk.ends_with('\n');
        self.any = true;
        match chunk.rfind('\n') {
            Some(at) => {
                self.last_line.clear();
                self.last_line.push_str(&chunk[at + 1..]);
            }
            None => self.last_line.push_str(chunk),
        }

        match &mut self.text {
            Some(text) => {
                text.push_str(chunk);
                self.chars = measure(text).chars;
                if verdict(self.size(), config) == ReadMode::TooBig {
                    self.text = None;
                }
            }
            None => self.chars = self.chars.saturating_add(chunk.chars().count()),
        }
    }

    /// Matches what [`measure`] would report for the whole span: exactly for lines, and
    /// for chars until the bytes are dropped.
    pub(crate) fn size(&self) -> TextSize {
        let trailing = usize::from(self.any && !self.ends_with_newline);
        TextSize {
            lines: self.newlines + trailing,
            chars: self.chars,
        }
    }

    /// The row the far end is still sitting on: everything since the last line ending.
    ///
    /// `None` when the span ends at one, and when what is outstanding is only
    /// whitespace: some shells draw a prompt across two rows and the first is blank.
    pub(crate) fn last_line(&self) -> Option<&str> {
        (!self.last_line.trim().is_empty()).then_some(self.last_line.as_str())
    }

    /// Takes the span, leaving the accumulator empty. The text is `None` when it was
    /// dropped.
    pub(crate) fn take(&mut self) -> (Option<String>, TextSize) {
        let size = self.size();
        let text = self.text.take();
        *self = Self::default();
        (text, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_all(chunks: &[&str], config: &PacingConfig) -> UnspokenText {
        let mut unspoken = UnspokenText::default();
        for chunk in chunks {
            unspoken.push(chunk, config);
        }
        unspoken
    }

    #[test]
    fn an_accumulated_span_measures_as_the_whole_text_would() {
        let config = PacingConfig::default();
        for chunks in [
            vec!["one\n", "two\n", "three"],
            vec!["no trailing newline"],
            vec!["a\n", "\n", "b\n"],
            vec!["split ", "across ", "chunks\n"],
        ] {
            let joined: String = chunks.concat();
            assert_eq!(
                push_all(&chunks, &config).size(),
                measure(&joined),
                "chunks {chunks:?}"
            );
        }
    }

    #[test]
    fn empty_chunks_change_nothing() {
        let config = PacingConfig::default();
        let mut unspoken = UnspokenText::default();
        unspoken.push("", &config);
        assert_eq!(unspoken.size(), TextSize { lines: 0, chars: 0 });

        unspoken.push("text\n", &config);
        let before = unspoken.size();
        unspoken.push("", &config);
        assert_eq!(unspoken.size(), before);
    }

    #[test]
    fn text_is_kept_while_it_could_still_be_spoken() {
        let config = PacingConfig::default();
        let unspoken = push_all(&["small\n", "enough\n"], &config);
        let (text, size) = { unspoken }.take();
        assert_eq!(text.as_deref(), Some("small\nenough\n"));
        assert_eq!(size.lines, 2);
    }

    #[test]
    fn text_is_dropped_once_the_verdict_is_settled_but_lines_stay_exact() {
        let config = PacingConfig::default();
        let mut unspoken = UnspokenText::default();
        for _ in 0..200 {
            unspoken.push("a line of output\n", &config);
        }
        let (text, size) = unspoken.take();
        assert_eq!(text, None, "bytes past the threshold are not worth holding");
        assert_eq!(
            size.lines, 200,
            "the announcement still needs an exact count"
        );
        assert!(verdict(size, &config) == ReadMode::TooBig);
    }

    #[test]
    fn a_flood_does_not_grow_without_bound() {
        let config = PacingConfig::default();
        let mut unspoken = UnspokenText::default();
        for _ in 0..10_000 {
            unspoken.push(&"y\n".repeat(100), &config);
        }
        assert_eq!(unspoken.text, None);
        assert_eq!(unspoken.size().lines, 1_000_000);
    }

    #[test]
    fn the_last_unterminated_row_survives_the_bytes_being_dropped() {
        let config = PacingConfig::default();
        let mut unspoken = UnspokenText::default();
        for _ in 0..200 {
            unspoken.push("a line of output\n", &config);
        }
        unspoken.push("marlon@ubuntu:~$ ", &config);

        assert_eq!(unspoken.last_line(), Some("marlon@ubuntu:~$ "));
        let (text, _) = unspoken.take();
        assert_eq!(text, None, "the span itself is still not worth holding");
    }

    #[test]
    fn a_row_arriving_in_pieces_is_one_row() {
        let config = PacingConfig::default();
        let mut unspoken = UnspokenText::default();
        unspoken.push("done\nmarlon", &config);
        unspoken.push("@ubuntu", &config);
        unspoken.push(":~$ ", &config);

        assert_eq!(unspoken.last_line(), Some("marlon@ubuntu:~$ "));
    }

    #[test]
    fn a_span_that_ends_at_a_line_ending_has_no_last_row() {
        let config = PacingConfig::default();
        let mut unspoken = UnspokenText::default();
        unspoken.push("one\ntwo\n", &config);

        assert_eq!(unspoken.last_line(), None);
    }

    #[test]
    fn a_blank_row_is_not_a_row() {
        let config = PacingConfig::default();
        let mut unspoken = UnspokenText::default();
        unspoken.push("one\n   ", &config);

        assert_eq!(unspoken.last_line(), None);
    }

    #[test]
    fn taking_the_span_takes_the_row_with_it() {
        let config = PacingConfig::default();
        let mut unspoken = UnspokenText::default();
        unspoken.push("one\n$ ", &config);
        let _ = unspoken.take();

        assert_eq!(unspoken.last_line(), None);
    }

    #[test]
    fn taking_resets_everything() {
        let config = PacingConfig::default();
        let mut unspoken = push_all(&["some\n", "text\n"], &config);
        let _ = unspoken.take();
        assert_eq!(unspoken.size(), TextSize { lines: 0, chars: 0 });
        assert_eq!(unspoken, UnspokenText::default());
    }
}
