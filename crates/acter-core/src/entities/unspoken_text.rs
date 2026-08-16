//! Entity/value: the text a running command has produced but not yet announced, with
//! the one invariant that keeps a session actor's memory bounded.
//!
//! Speech is the only reason to keep this text: it has already been rendered and is
//! reviewable in the buffer (DESIGN, buffer and speech are separate paths). So once the
//! accumulated span passes the auto-read threshold its verdict is settled as
//! [`ReadMode::TooBig`] forever — no later chunk can bring it back under — and a too-big
//! announcement needs only the line count. From that point the bytes are dropped and
//! only counts are kept.
//!
//! That is what bounds the gapless-flood case (`yes`, a busy `tail -f`) that no pacing
//! rule reaches: under a flood no quiescent gap ever occurs, so without this nothing
//! would ever be flushed and nothing ever freed.

use crate::PacingConfig;
use crate::entities::ReadMode;
use crate::policies::{TextSize, measure, verdict};

/// Unannounced text for one command. Line counts stay exact whether or not the text is
/// still held; the character count is exact only while it is, and afterwards is an
/// over-estimate that stays above `max_chars` — which is all the threshold needs, since
/// the verdict is already settled.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct UnspokenText {
    /// `None` once the verdict is settled as too big and the bytes were dropped — which
    /// is why this cannot derive `Default`: an empty accumulator holds an empty string,
    /// not nothing.
    text: Option<String>,
    newlines: usize,
    chars: usize,
    ends_with_newline: bool,
    any: bool,
}

impl Default for UnspokenText {
    fn default() -> Self {
        Self {
            text: Some(String::new()),
            newlines: 0,
            chars: 0,
            ends_with_newline: false,
            any: false,
        }
    }
}

impl UnspokenText {
    /// Adds a chunk. Empty chunks change nothing — they are not output (B1.1).
    pub(crate) fn push(&mut self, chunk: &str, config: &PacingConfig) {
        if chunk.is_empty() {
            return;
        }
        self.newlines += chunk.matches('\n').count();
        self.ends_with_newline = chunk.ends_with('\n');
        self.any = true;

        match &mut self.text {
            Some(text) => {
                text.push_str(chunk);
                self.chars = measure(text).chars;
                if verdict(self.size(), config) == ReadMode::TooBig {
                    // The verdict can never come back under the threshold, and a
                    // too-big announcement carries only the line count.
                    self.text = None;
                }
            }
            // Already over: keep counting, and let `chars` over-estimate. Raw chars are
            // never fewer than the trimmed measure, so it stays above `max_chars`.
            None => self.chars = self.chars.saturating_add(chunk.chars().count()),
        }
    }

    /// The measured size, matching what [`measure`] would report for the whole span —
    /// exactly for lines, and for chars until the bytes are dropped.
    pub(crate) fn size(&self) -> TextSize {
        let trailing = usize::from(self.any && !self.ends_with_newline);
        TextSize {
            lines: self.newlines + trailing,
            chars: self.chars,
        }
    }

    /// Takes the span, leaving the accumulator empty. The text is `None` when it was
    /// dropped — a caller announcing a too-big verdict needs only [`TextSize::lines`].
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
        // No quiescent gap ever comes, so nothing is ever flushed: the accumulator has
        // to survive this on its own.
        for _ in 0..10_000 {
            unspoken.push(&"y\n".repeat(100), &config);
        }
        assert_eq!(unspoken.text, None);
        assert_eq!(unspoken.size().lines, 1_000_000);
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
