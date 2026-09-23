//! Entity/value: the text a running command has produced but not yet announced.
//!
//! Past the auto-read threshold the verdict cannot change back, since the size only grows,
//! so the text is dropped and only counts are kept; that bounds memory under a gapless flood
//! such as `yes`.

use crate::PacingConfig;
use crate::entities::ReadMode;
use crate::policies::{TextSize, measure, verdict};

/// Once the text is dropped the character count is an over-estimate; line counts stay exact.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct UnspokenText {
    /// `None` once dropped; empty is `Some("")`, hence the manual `Default`.
    text: Option<String>,
    newlines: usize,
    chars: usize,
    ends_with_newline: bool,
    any: bool,
    /// Kept after the text is dropped; in an unintegrated session this row is the prompt.
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

    pub(crate) fn size(&self) -> TextSize {
        let trailing = usize::from(self.any && !self.ends_with_newline);
        TextSize {
            lines: self.newlines + trailing,
            chars: self.chars,
        }
    }

    /// `None` when the span ends at a line ending or the row is only whitespace, as the
    /// blank first row of a two-row prompt is.
    pub(crate) fn last_line(&self) -> Option<&str> {
        (!self.last_line.trim().is_empty()).then_some(self.last_line.as_str())
    }

    /// The text is `None` when it was dropped.
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
