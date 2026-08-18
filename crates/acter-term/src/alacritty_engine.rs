//! Adapter: the [`TerminalEngine`] implementation over `alacritty_terminal`. Bytes in;
//! identified lines of extracted text, recognized OSC 133 markers and screen transitions
//! out, as one ordered stream.
//!
//! **Two parsers over the same bytes, and nothing forwards.** One drives a real `Term`
//! with zero forwarding; the other drives a [`Sniffer`] whose entire job is stream
//! position. The alternative — one parser and a wrapper owning a `Term`, forwarding all
//! seventy-two `Handler` methods — works today and fails quietly later, for the reason
//! written up in [`sniffer`]. The cost accepted knowingly is one extra pass over each
//! chunk, which is cheap next to the grid mutation the same bytes cause in `Term`.
//!
//! One residual, recorded rather than discovered: a parser holds synchronized-update
//! (DCS 2026) timeout state, so the two instances could in principle flush buffered
//! output at different moments. Both are advanced over the same slice back to back, so
//! the window is microseconds against a timeout measured in hundreds of milliseconds,
//! and it self-corrects on the next chunk.
//!
//! **Position comes from the sniffer; state comes from the emulator.** `Term::mode()`
//! remains the authority on which screen is current, and [`AlacrittyEngine::screen`]
//! reports it that way. What the emulator cannot supply is *where* in the stream a
//! switch happened, and that matters: a single read routinely carries `ESC[?1049h`
//! followed by an application's first full repaint, so polling the mode after the batch
//! would attribute a screenful of `vim` chrome to the command the user just finished
//! (spec B3, decision 2).

mod extractor;
mod listener;
mod sniffer;

use std::mem::take;

use acter_core::{Osc133Marker, Screen, TerminalEngine, TerminalItem};
use alacritty_terminal::Term;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Osc52, TermMode};
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

use extractor::Extractor;
use listener::DeviceReplies;
use sniffer::{Signal, Sniffer};

/// How many rows of scrolled-off output the emulator stages between extractions.
///
/// Not the user's scrollback: the extractor reclaims this after every scan, so it only
/// has to hold the rows a single read can scroll away before anyone has looked at them.
/// Sized far above that so the gap sentence stays a genuine last resort.
const STAGING_ROWS: usize = 10_000;

/// One session's terminal emulator.
pub struct AlacrittyEngine {
    term: Term<DeviceReplies>,
    term_parser: Processor<StdSyncHandler>,
    sniffer: Sniffer,
    sniffer_parser: Processor<StdSyncHandler>,
    replies: DeviceReplies,
    extractor: Extractor,
    /// Items produced outside [`TerminalEngine::advance`]. A resize settles the lines it
    /// invalidates and has nowhere to return them, so they ride out with the next batch
    /// rather than being dropped.
    pending: Vec<TerminalItem>,
}

impl AlacrittyEngine {
    pub fn new(columns: u16, screen_lines: u16) -> Self {
        Self::with_staging_rows(columns, screen_lines, STAGING_ROWS)
    }

    fn with_staging_rows(columns: u16, screen_lines: u16, staging_rows: usize) -> Self {
        let size = TermSize::new(
            usize::from(columns.max(1)),
            usize::from(screen_lines.max(1)),
        );
        let config = Config {
            scrolling_history: staging_rows,
            // Set explicitly rather than left at the crate default, which accepts copy
            // requests: this terminal has no clipboard story yet, so it takes part in
            // none of OSC 52.
            osc52: Osc52::Disabled,
            ..Default::default()
        };
        let replies = DeviceReplies::new(columns.max(1), screen_lines.max(1));

        Self {
            term: Term::new(config, &size, replies.clone()),
            term_parser: Processor::new(),
            sniffer: Sniffer::default(),
            sniffer_parser: Processor::new(),
            replies,
            extractor: Extractor::new(staging_rows),
            pending: Vec::new(),
        }
    }

    /// Places one sniffed signal in the stream, and reports whether the grid's row
    /// numbering is about to stop meaning what it meant.
    fn place(&mut self, signal: Signal, items: &mut Vec<TerminalItem>) -> bool {
        match signal {
            // Only a command end freezes a block's lines. A prompt start or a command
            // start would freeze the row the prompt is drawn on, which is the same row
            // the shell then echoes the command onto — so the echo would arrive as a
            // second line repeating the prompt.
            Signal::Marker(marker) => {
                if matches!(marker, Osc133Marker::CommandEnd(_)) {
                    self.extractor.settle_block(&self.term, items);
                }
                items.push(TerminalItem::Marker(marker));
                false
            }
            // Settled from the grid that is still current: the emulator has not swapped
            // yet, so the lines are read as they were on the screen being left. The rows
            // are forgotten outright rather than frozen, because the screen arriving is a
            // separate grid whose coordinates mean nothing to the one being left.
            Signal::ScreenChanged(screen) => {
                self.extractor.settle_and_forget(&self.term, items);
                items.push(TerminalItem::ScreenChanged(screen));
                true
            }
        }
    }
}

impl TerminalEngine for AlacrittyEngine {
    /// Advances both parsers over the batch, cutting the emulator's stream wherever the
    /// sniffer found something to place.
    ///
    /// The sniffer runs a byte at a time because a `Handler` call says *what* happened
    /// and not *where*; stepping it is how the offset becomes known. The emulator, which
    /// is the expensive one, still gets whole slices. Its own bytes are fed after the
    /// text that preceded them, so a screen change is placed while the grid still holds
    /// the screen being left.
    fn advance(&mut self, bytes: &[u8]) -> Vec<TerminalItem> {
        let mut items = take(&mut self.pending);
        let mut segment = 0;

        for index in 0..bytes.len() {
            self.sniffer_parser
                .advance(&mut self.sniffer, &bytes[index..=index]);
            if !self.sniffer.signalled() {
                continue;
            }

            // Everything up to, but not including, the byte that completed the sequence.
            // The sequence's own bytes print nothing, so the grid is already in its
            // pre-sequence state.
            self.term_parser
                .advance(&mut self.term, &bytes[segment..index]);
            self.extractor.extract(&mut self.term, &mut items);

            let mut renumbered = false;
            for signal in self.sniffer.drain() {
                renumbered |= self.place(signal, &mut items);
            }

            self.term_parser
                .advance(&mut self.term, &bytes[index..=index]);
            if renumbered {
                self.extractor.reanchor(&self.term);
            }
            segment = index + 1;
        }

        self.term_parser.advance(&mut self.term, &bytes[segment..]);
        self.extractor.extract(&mut self.term, &mut items);
        items
    }

    fn screen(&self) -> Screen {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            Screen::Alternate
        } else {
            Screen::Normal
        }
    }

    /// Resizing reflows the grid, so every row number the extractor holds stops meaning
    /// what it meant. The open lines are settled first and their ids retired.
    fn resize(&mut self, columns: u16, screen_lines: u16) {
        let columns = columns.max(1);
        let screen_lines = screen_lines.max(1);

        self.extractor
            .settle_and_forget(&self.term, &mut self.pending);
        self.term.resize(TermSize::new(
            usize::from(columns),
            usize::from(screen_lines),
        ));
        self.extractor.reanchor(&self.term);
        self.replies.resized(columns, screen_lines);
    }

    fn take_replies(&mut self) -> Vec<u8> {
        self.replies.take()
    }
}

#[cfg(test)]
mod tests {
    use acter_core::{ExitCode, LineId, LineRevision};
    use proptest::prelude::*;

    use super::extractor::grid_lines;
    use super::*;

    const APPENDED: LineRevision = LineRevision::Appended;
    const REWRITTEN: LineRevision = LineRevision::Rewritten;
    const SETTLED: LineRevision = LineRevision::Settled;

    fn engine() -> AlacrittyEngine {
        AlacrittyEngine::new(20, 5)
    }

    fn line(id: u64, text: &str, revision: LineRevision) -> TerminalItem {
        TerminalItem::Line {
            id: LineId(id),
            text: text.to_owned(),
            revision,
        }
    }

    fn marker(marker: Osc133Marker) -> TerminalItem {
        TerminalItem::Marker(marker)
    }

    /// Just the line items, as tuples, for tests that do not care about markers.
    fn lines(items: &[TerminalItem]) -> Vec<(u64, String, LineRevision)> {
        items
            .iter()
            .filter_map(|item| match item {
                TerminalItem::Line { id, text, revision } => Some((id.0, text.clone(), *revision)),
                _ => None,
            })
            .collect()
    }

    fn texts(items: &[TerminalItem]) -> Vec<String> {
        lines(items).into_iter().map(|(_, text, _)| text).collect()
    }

    #[test]
    fn the_marker_cycle_arrives_as_one_ordered_stream() {
        let mut engine = AlacrittyEngine::new(40, 5);
        let items = engine.advance(
            b"\x1b]133;A\x07prompt$ \x1b]133;B\x07echo hi\r\n\x1b]133;C\x07hi\r\n\x1b]133;D;0\x07",
        );

        assert_eq!(
            items,
            vec![
                marker(Osc133Marker::PromptStart),
                line(0, "prompt$", APPENDED),
                marker(Osc133Marker::CommandStart),
                line(0, " echo hi", APPENDED),
                marker(Osc133Marker::OutputStart),
                line(1, "hi", APPENDED),
                line(0, "prompt$ echo hi", SETTLED),
                line(1, "hi", SETTLED),
                marker(Osc133Marker::CommandEnd(Some(ExitCode(0)))),
            ]
        );
    }

    #[test]
    fn a_marker_split_across_two_reads_is_still_recognized() {
        let mut engine = engine();
        let first = engine.advance(b"done\r\n\x1b]133;D;");
        let second = engine.advance(b"0\x07");

        assert!(
            !first
                .iter()
                .any(|item| matches!(item, TerminalItem::Marker(_)))
        );
        assert!(second.contains(&marker(Osc133Marker::CommandEnd(Some(ExitCode(0))))));
    }

    #[test]
    fn a_plain_line_is_extracted_without_its_padding() {
        let mut engine = engine();

        assert_eq!(
            engine.advance(b"hello   \r\n"),
            vec![line(0, "hello", APPENDED)]
        );
    }

    #[test]
    fn a_line_finished_in_a_later_read_is_not_duplicated() {
        let mut engine = engine();
        let first = engine.advance(b"hel");
        let second = engine.advance(b"lo\r\n");

        assert_eq!(first, vec![line(0, "hel", APPENDED)]);
        assert_eq!(second, vec![line(0, "lo", APPENDED)]);
    }

    #[test]
    fn a_line_longer_than_the_grid_arrives_as_one_logical_line() {
        let mut engine = AlacrittyEngine::new(10, 5);
        let items = engine.advance(b"0123456789abc\r\n");

        assert_eq!(texts(&items), vec!["0123456789abc"]);
    }

    #[test]
    fn wide_characters_are_not_doubled() {
        let mut engine = AlacrittyEngine::new(10, 5);
        let items = engine.advance("日本語\r\n".as_bytes());

        assert_eq!(texts(&items), vec!["日本語"]);
    }

    #[test]
    fn a_combining_accent_survives_extraction() {
        let mut engine = engine();
        let items = engine.advance("cafe\u{0301}\r\n".as_bytes());

        assert_eq!(texts(&items), vec!["cafe\u{0301}"]);
    }

    #[test]
    fn a_blank_line_between_two_others_is_preserved() {
        let mut engine = engine();
        let items = engine.advance(b"first\r\n\r\nlast\r\n");

        assert_eq!(texts(&items), vec!["first", "", "last"]);
    }

    #[test]
    fn the_row_the_cursor_just_moved_onto_is_not_a_line_yet() {
        let mut engine = engine();

        assert_eq!(engine.advance(b"only\r\n"), vec![line(0, "only", APPENDED)]);
    }

    #[test]
    fn a_carriage_return_rewrite_carries_the_whole_line() {
        let mut engine = engine();
        engine.advance(b"downloading");
        let items = engine.advance(b"\rdone       ");

        assert_eq!(items, vec![line(0, "done", REWRITTEN)]);
    }

    #[test]
    fn clearing_a_line_is_a_rewrite() {
        let mut engine = engine();
        engine.advance(b"downloading");
        let items = engine.advance(b"\r\x1b[2K");

        assert_eq!(items, vec![line(0, "", REWRITTEN)]);
    }

    #[test]
    fn erasing_characters_is_a_rewrite() {
        let mut engine = engine();
        engine.advance(b"abcdef");
        let items = engine.advance(b"\r\x1b[3X");

        assert_eq!(items, vec![line(0, "   def", REWRITTEN)]);
    }

    #[test]
    fn a_line_scrolling_out_of_the_screen_area_settles() {
        let mut engine = AlacrittyEngine::new(20, 3);
        let items = engine.advance(b"a\r\nb\r\nc\r\nd\r\n");

        assert_eq!(
            lines(&items),
            vec![
                (0, "a".to_owned(), SETTLED),
                (1, "b".to_owned(), SETTLED),
                (2, "c".to_owned(), APPENDED),
                (3, "d".to_owned(), APPENDED),
            ]
        );
    }

    #[test]
    fn a_block_closing_marker_settles_open_lines_before_the_marker() {
        let mut engine = engine();
        let items = engine.advance(b"output\r\n\x1b]133;D;0\x07");

        assert_eq!(
            items,
            vec![
                line(0, "output", APPENDED),
                line(0, "output", SETTLED),
                marker(Osc133Marker::CommandEnd(Some(ExitCode(0)))),
            ]
        );
    }

    #[test]
    fn a_screen_change_settles_open_lines_before_the_transition() {
        let mut engine = engine();
        let items = engine.advance(b"before\r\n\x1b[?1049hafter\r\n");

        assert_eq!(
            items,
            vec![
                line(0, "before", APPENDED),
                line(0, "before", SETTLED),
                TerminalItem::ScreenChanged(Screen::Alternate),
                line(1, "after", APPENDED),
            ]
        );
        assert_eq!(engine.screen(), Screen::Alternate);
    }

    #[test]
    fn leaving_the_alternate_screen_reports_normal_again() {
        let mut engine = engine();
        engine.advance(b"\x1b[?1049h");
        let items = engine.advance(b"\x1b[?1049l");

        assert!(items.contains(&TerminalItem::ScreenChanged(Screen::Normal)));
        assert_eq!(engine.screen(), Screen::Normal);
    }

    #[test]
    fn a_spinner_emits_many_rewrites_and_exactly_one_settlement() {
        let mut engine = engine();
        let mut items = engine.advance(b"working |");
        for frame in [b"/", b"-", b"\\", b"|", b"/", b"-", b"\\"] {
            items.extend(engine.advance(b"\rworking "));
            items.extend(engine.advance(frame));
        }
        items.extend(engine.advance(b"\rdone     "));
        items.extend(engine.advance(b"\x1b]133;D;0\x07"));

        let revisions: Vec<_> = lines(&items)
            .into_iter()
            .map(|(_, _, revision)| revision)
            .collect();
        assert_eq!(revisions.iter().filter(|r| **r == SETTLED).count(), 1);
        assert!(revisions.iter().filter(|r| **r == REWRITTEN).count() > 4);
        assert_eq!(
            lines(&items).last().map(|(_, text, _)| text.clone()),
            Some("done".to_owned())
        );
    }

    #[test]
    fn a_rewrite_after_the_block_closed_starts_a_new_line() {
        let mut engine = engine();
        engine.advance(b"progress");
        engine.advance(b"\x1b]133;D;0\x07");
        let items = engine.advance(b"\rdone    ");

        assert_eq!(lines(&items), vec![(1, "done".to_owned(), APPENDED)]);
    }

    #[test]
    fn a_resize_settles_the_open_lines_and_re_mints() {
        let mut engine = engine();
        engine.advance(b"hello");
        engine.resize(40, 5);
        let items = engine.advance(b"!");

        assert_eq!(
            lines(&items),
            vec![
                (0, "hello".to_owned(), SETTLED),
                (1, "hello!".to_owned(), APPENDED),
            ]
        );
    }

    #[test]
    fn a_multi_row_in_place_update_revises_each_row_under_its_own_id() {
        let mut engine = AlacrittyEngine::new(20, 6);
        engine.advance(b"layer1: pulling\r\nlayer2: pulling\r\n");
        let items = engine.advance(b"\x1b[2A\rlayer1: done   \r\n\rlayer2: done   \r\n");

        assert_eq!(
            lines(&items),
            vec![
                (0, "layer1: done".to_owned(), REWRITTEN),
                (1, "layer2: done".to_owned(), REWRITTEN),
            ]
        );
    }

    #[test]
    fn escape_sequences_never_reach_the_text() {
        let mut engine = AlacrittyEngine::new(40, 5);
        let items = engine.advance(b"\x1b[?25l\x1b[31mred\x1b[0m\x1b[1m bold\x1b[m\x1b[?25h\r\n");

        assert_eq!(texts(&items), vec!["red bold"]);
    }

    #[test]
    fn a_device_query_produces_replies_that_are_handed_out_once() {
        let mut engine = engine();
        engine.advance(b"\x1b[6n");

        assert!(!engine.take_replies().is_empty());
        assert!(engine.take_replies().is_empty());
    }

    #[test]
    fn scrollback_overflow_is_announced_rather_than_silent() {
        let mut engine = AlacrittyEngine::with_staging_rows(20, 3, 4);
        let mut transcript = Vec::new();
        for index in 0..40 {
            transcript.extend_from_slice(format!("line {index}\r\n").as_bytes());
        }
        let items = engine.advance(&transcript);

        assert!(
            texts(&items)
                .iter()
                .any(|text| text.starts_with("Some output was lost")),
            "the gap must be spoken, not skipped: {:?}",
            texts(&items)
        );
    }

    /// Replays a stream the way a correct consumer would: append a delta, replace on a
    /// rewrite or a settlement, keyed by id and kept in the order lines first appeared.
    fn replay(items: &[TerminalItem]) -> Vec<String> {
        let mut ids: Vec<LineId> = Vec::new();
        let mut texts: Vec<String> = Vec::new();
        for item in items {
            let TerminalItem::Line { id, text, revision } = item else {
                continue;
            };
            let index = match ids.iter().position(|known| known == id) {
                Some(index) => index,
                None => {
                    ids.push(*id);
                    texts.push(String::new());
                    ids.len() - 1
                }
            };
            match revision {
                LineRevision::Appended => texts[index].push_str(text),
                LineRevision::Rewritten | LineRevision::Settled => texts[index] = text.clone(),
            }
        }
        while texts.last().is_some_and(|text| text.is_empty()) {
            texts.pop();
        }
        texts
    }

    /// Blank lines are dropped before the two equality properties compare, because one
    /// case genuinely cannot be expressed: when a line grows past the right margin onto a
    /// row that already held a line of its own, that line stops existing, and the stream
    /// has no item for "this line is gone" — it settles empty instead. Blank-line
    /// preservation is table-tested above, where it can be stated exactly.
    fn without_blanks(lines: Vec<String>) -> Vec<String> {
        lines.into_iter().filter(|line| !line.is_empty()).collect()
    }

    /// Everything the same bytes leave in a grid that keeps all of its history — the
    /// reference the emitted stream is measured against.
    fn reference_lines(bytes: &[u8], columns: usize, screen_lines: usize) -> Vec<String> {
        let config = Config {
            scrolling_history: 100_000,
            osc52: Osc52::Disabled,
            ..Default::default()
        };
        let replies = DeviceReplies::new(columns as u16, screen_lines as u16);
        let mut term = Term::new(config, &TermSize::new(columns, screen_lines), replies);
        let mut parser = Processor::<StdSyncHandler>::new();
        parser.advance(&mut term, bytes);
        grid_lines(&term)
    }

    fn drive(bytes: &[u8], chunk: usize) -> Vec<TerminalItem> {
        let mut engine = AlacrittyEngine::with_staging_rows(12, 4, 512);
        let mut items = Vec::new();
        for slice in bytes.chunks(chunk.max(1)) {
            items.extend(engine.advance(slice));
        }
        items
    }

    /// Fragments a terminal actually emits. Screen swaps, resizes and block-closing
    /// markers are deliberately absent from the transcripts used for the two equality
    /// properties: each one retires the ids it settles, so a later rewrite of the same
    /// rows becomes a *new* line — the duplication decision 7 accepts on purpose, and
    /// which an equality against the final grid cannot express. Both are covered by
    /// table tests above, and the panic property below takes genuinely arbitrary bytes.
    fn any_fragment() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            "[a-z ]{0,10}".prop_map(String::into_bytes),
            Just(b"\r\n".to_vec()),
            Just(b"\n".to_vec()),
            Just(b"\r".to_vec()),
            Just(b"\t".to_vec()),
            Just(b"\x08".to_vec()),
            Just(b"\x1b[31m".to_vec()),
            Just(b"\x1b[0m".to_vec()),
            Just(b"\x1b[A".to_vec()),
            Just(b"\x1b[B".to_vec()),
            Just(b"\x1b[K".to_vec()),
            Just(b"\x1b[2K".to_vec()),
            Just(b"\x1b[3X".to_vec()),
            Just(b"\x1b[5G".to_vec()),
        ]
    }

    fn any_transcript() -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(any_fragment(), 0..24).prop_map(|parts| parts.concat())
    }

    fn any_bytes() -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(any::<u8>(), 0..256)
    }

    proptest! {
        /// Moved here from B2 by B2's own decision: the property belongs wherever bytes
        /// are actually parsed, and this is the entry that parses them.
        #[test]
        fn never_panics_on_arbitrary_bytes(chunks in prop::collection::vec(any_bytes(), 0..4)) {
            let mut engine = AlacrittyEngine::with_staging_rows(12, 4, 64);
            for chunk in chunks {
                let _ = engine.advance(&chunk);
            }
            let _ = engine.take_replies();
            let _ = engine.screen();
        }

        /// B2's cardinal property, re-expressed for identified lines: replaying the
        /// stream reconstructs exactly the text the terminal finished with, in row
        /// order. Equality, not "at least once" — revision removed the duplication that
        /// would have forced the weaker statement.
        #[test]
        fn no_line_is_ever_lost(transcript in any_transcript(), chunk in 1usize..40) {
            let items = drive(&transcript, chunk);

            prop_assert_eq!(
                without_blanks(replay(&items)),
                without_blanks(reference_lines(&transcript, 12, 4))
            );
        }

        /// The individual items depend on where the reads fell — a line emitted in one
        /// piece or three — but what they reconstruct to does not.
        #[test]
        fn reconstruction_is_independent_of_chunking(transcript in any_transcript()) {
            let whole = without_blanks(replay(&drive(&transcript, transcript.len().max(1))));
            for chunk in [1, 3, 7, 29] {
                let chunked = without_blanks(replay(&drive(&transcript, chunk)));
                prop_assert_eq!(chunked, whole.clone());
            }
        }

        /// A settlement is a line's last word, so nothing may follow it and no line may
        /// settle twice. Markers are included here, because settling is exactly what
        /// they trigger.
        #[test]
        fn every_id_settles_at_most_once_and_nothing_follows_it(
            transcript in any_transcript(),
            chunk in 1usize..40,
        ) {
            let mut engine = AlacrittyEngine::with_staging_rows(12, 4, 64);
            let mut items = Vec::new();
            for slice in transcript.chunks(chunk) {
                items.extend(engine.advance(slice));
            }
            items.extend(engine.advance(b"\x1b]133;D;0\x07"));

            let mut settled: Vec<LineId> = Vec::new();
            for item in &items {
                let TerminalItem::Line { id, revision, .. } = item else {
                    continue;
                };
                prop_assert!(!settled.contains(id), "an item followed a settled line");
                if *revision == LineRevision::Settled {
                    settled.push(*id);
                }
            }
        }
    }
}
