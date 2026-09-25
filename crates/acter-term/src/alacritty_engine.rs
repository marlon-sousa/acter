//! Adapter: the [`TerminalEngine`] implementation over `alacritty_terminal`.
//!
//! A single read routinely carries `ESC[?1049h` followed by an application's first full
//! repaint, so a screen switch is placed at its byte offset in the stream, not read from
//! `Term::mode()` after the batch.

mod extractor;
mod listener;
mod sniffer;

use std::mem::take;

use acter_core::{Cursor, Osc133Marker, Screen, TerminalEngine, TerminalItem, TerminalModes};
use alacritty_terminal::Term;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Osc52, TermMode};
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

use extractor::Extractor;
use listener::DeviceReplies;
use sniffer::{Signal, Sniffer};

/// Rows of history the emulator stages between extractions, not the user's scrollback.
const STAGING_ROWS: usize = 10_000;

/// One session's terminal emulator.
pub struct AlacrittyEngine {
    term: Term<DeviceReplies>,
    term_parser: Processor<StdSyncHandler>,
    sniffer: Sniffer,
    sniffer_parser: Processor<StdSyncHandler>,
    replies: DeviceReplies,
    extractor: Extractor,
    /// Lines a resize settled, returned with the next batch.
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

    /// Returns the screen being switched to, or `None` when the row numbering still holds.
    fn place(&mut self, signal: Signal, items: &mut Vec<TerminalItem>) -> Option<Screen> {
        match signal {
            // Settling on a prompt or command start would freeze the prompt's row, and the
            // echoed command would then arrive as a second line repeating the prompt.
            Signal::Marker(marker) => {
                if matches!(marker, Osc133Marker::CommandEnd(_)) {
                    self.extractor.settle_block(&self.term, items);
                }
                items.push(TerminalItem::Marker(marker));
                None
            }
            // The emulator has not swapped yet, so these lines are read from the screen being left.
            Signal::ScreenChanged(screen) => {
                self.extractor.settle_and_forget(&self.term, items);
                items.push(TerminalItem::ScreenChanged(screen));
                Some(screen)
            }
        }
    }
}

impl TerminalEngine for AlacrittyEngine {
    fn advance(&mut self, bytes: &[u8]) -> Vec<TerminalItem> {
        let mut items = take(&mut self.pending);
        let mut segment = 0;

        for index in 0..bytes.len() {
            self.sniffer_parser
                .advance(&mut self.sniffer, &bytes[index..=index]);
            if !self.sniffer.signalled() {
                continue;
            }

            // Outside a synchronized update the sequence's own bytes print nothing, so the grid
            // is still in its pre-sequence state.
            self.term_parser
                .advance(&mut self.term, &bytes[segment..index]);
            self.extractor.extract(&mut self.term, &mut items);

            let mut renumbered = None;
            for signal in self.sniffer.drain() {
                // The last screen change wins: it is the grid the emulator ends up on.
                renumbered = self.place(signal, &mut items).or(renumbered);
            }

            self.term_parser
                .advance(&mut self.term, &bytes[index..=index]);
            match renumbered {
                // The alternate screen arrives blank and is painted from its top row; the normal
                // screen comes back holding text that was already emitted.
                Some(Screen::Alternate) => self.extractor.reanchor_to_top(&self.term),
                Some(Screen::Normal) => self.extractor.reanchor(&self.term),
                None => {}
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

    fn cursor(&self) -> Cursor {
        let point = self.term.grid().cursor.point;
        Cursor {
            column: u16::try_from(point.column.0).unwrap_or(u16::MAX),
            row: u16::try_from(point.line.0.max(0)).unwrap_or(u16::MAX),
            visible: self.term.mode().contains(TermMode::SHOW_CURSOR),
        }
    }

    fn modes(&self) -> TerminalModes {
        let mode = self.term.mode();
        TerminalModes {
            application_cursor_keys: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        }
    }
}

#[cfg(test)]
mod tests {
    use acter_core::{Colour, ExitCode, LineId, LineRevision, Style, StyleRun, join_runs};
    use proptest::prelude::*;

    use super::extractor::{grid_lines, styled_grid_lines};
    use super::*;

    const APPENDED: LineRevision = LineRevision::Appended;
    const REWRITTEN: LineRevision = LineRevision::Rewritten;
    const SETTLED: LineRevision = LineRevision::Settled;

    fn engine() -> AlacrittyEngine {
        AlacrittyEngine::new(20, 5)
    }

    fn line(id: u64, text: &str, revision: LineRevision) -> TerminalItem {
        styled(id, text, revision, vec![])
    }

    fn styled(id: u64, text: &str, revision: LineRevision, runs: Vec<StyleRun>) -> TerminalItem {
        TerminalItem::Line {
            id: LineId(id),
            text: text.to_owned(),
            revision,
            runs,
        }
    }

    fn marker(marker: Osc133Marker) -> TerminalItem {
        TerminalItem::Marker(marker)
    }

    fn lines(items: &[TerminalItem]) -> Vec<(u64, String, LineRevision)> {
        items
            .iter()
            .filter_map(|item| match item {
                TerminalItem::Line {
                    id, text, revision, ..
                } => Some((id.0, text.clone(), *revision)),
                _ => None,
            })
            .collect()
    }

    fn texts(items: &[TerminalItem]) -> Vec<String> {
        lines(items).into_iter().map(|(_, text, _)| text).collect()
    }

    #[test]
    fn a_sequence_an_interrupt_cut_short_does_not_swallow_the_prompt_after_it() {
        let mut engine = AlacrittyEngine::new(40, 5);

        let items = engine
            .advance(b"\x1b]133;D^C\r\n\x1b]133;A\x07prompt$ \x1b]133;B\x07next\r\n\x1b]133;C\x07");

        let markers: Vec<&TerminalItem> = items
            .iter()
            .filter(|item| matches!(item, TerminalItem::Marker(_)))
            .collect();
        assert_eq!(
            markers,
            vec![
                &marker(Osc133Marker::PromptStart),
                &marker(Osc133Marker::CommandStart),
                &marker(Osc133Marker::OutputStart),
            ],
            "the prompt after the truncated sequence is recognized in full: {items:?}"
        );
    }

    #[test]
    fn a_truncated_sequence_recovers_the_same_way_arriving_byte_by_byte() {
        let mut engine = AlacrittyEngine::new(40, 5);
        let stream: &[u8] =
            b"\x1b]133;D^C\r\n\x1b]133;A\x07prompt$ \x1b]133;B\x07next\r\n\x1b]133;C\x07";

        let mut markers = Vec::new();
        for byte in stream {
            for item in engine.advance(&[*byte]) {
                if let TerminalItem::Marker(found) = item {
                    markers.push(found);
                }
            }
        }

        assert_eq!(
            markers,
            vec![
                Osc133Marker::PromptStart,
                Osc133Marker::CommandStart,
                Osc133Marker::OutputStart,
            ]
        );
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
                line(1, "", APPENDED),
                line(2, "after", APPENDED),
            ]
        );
        assert_eq!(engine.screen(), Screen::Alternate);
    }

    #[test]
    fn a_repaint_from_the_top_of_the_alternate_screen_loses_no_rows() {
        let mut engine = engine();
        engine.advance(b"first\r\nsecond\r\n");
        let items = engine.advance(b"\x1b[?1049h\x1b[H\x1b[2J  a title bar\r\nthe first line\r\n");

        let painted = items
            .iter()
            .position(|item| *item == TerminalItem::ScreenChanged(Screen::Alternate))
            .expect("the switch is in the stream");
        assert_eq!(
            texts(&items[painted + 1..]),
            ["  a title bar", "the first line"],
            "every painted row arrives, in the order it was painted"
        );
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

    fn replay(items: &[TerminalItem]) -> Vec<String> {
        replay_styled(items)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    fn replay_styled(items: &[TerminalItem]) -> Vec<(String, Vec<StyleRun>)> {
        let mut ids: Vec<LineId> = Vec::new();
        let mut lines: Vec<(String, Vec<StyleRun>)> = Vec::new();
        for item in items {
            let TerminalItem::Line {
                id,
                text,
                revision,
                runs,
            } = item
            else {
                continue;
            };
            let index = match ids.iter().position(|known| known == id) {
                Some(index) => index,
                None => {
                    ids.push(*id);
                    lines.push((String::new(), Vec::new()));
                    ids.len() - 1
                }
            };
            let (shown, shown_runs) = &mut lines[index];
            match revision {
                LineRevision::Appended => {
                    join_runs(shown_runs, shown, runs);
                    shown.push_str(text);
                }
                LineRevision::Rewritten | LineRevision::Settled => {
                    *shown = text.clone();
                    *shown_runs = runs.clone();
                }
            }
        }
        while lines.last().is_some_and(|(text, _)| text.is_empty()) {
            lines.pop();
        }
        lines
    }

    fn without_blank_lines(lines: Vec<(String, Vec<StyleRun>)>) -> Vec<(String, Vec<StyleRun>)> {
        lines
            .into_iter()
            .filter(|(text, _)| !text.is_empty())
            .collect()
    }

    /// A line swallowed by a wrapping line above it settles empty (see `Extractor::absorb`), so
    /// blank lines cannot be compared exactly.
    fn without_blanks(lines: Vec<String>) -> Vec<String> {
        lines.into_iter().filter(|line| !line.is_empty()).collect()
    }

    fn reference_lines(bytes: &[u8], columns: usize, screen_lines: usize) -> Vec<String> {
        grid_lines(&reference_term(bytes, columns, screen_lines))
    }

    fn reference_styled_lines(bytes: &[u8]) -> Vec<(String, Vec<StyleRun>)> {
        styled_grid_lines(&reference_term(bytes, 12, 4))
    }

    fn reference_term(bytes: &[u8], columns: usize, screen_lines: usize) -> Term<DeviceReplies> {
        let config = Config {
            scrolling_history: 100_000,
            osc52: Osc52::Disabled,
            ..Default::default()
        };
        let replies = DeviceReplies::new(columns as u16, screen_lines as u16);
        let mut term = Term::new(config, &TermSize::new(columns, screen_lines), replies);
        let mut parser = Processor::<StdSyncHandler>::new();
        parser.advance(&mut term, bytes);
        term
    }

    fn drive(bytes: &[u8], chunk: usize) -> Vec<TerminalItem> {
        let mut engine = AlacrittyEngine::with_staging_rows(12, 4, 512);
        let mut items = Vec::new();
        for slice in bytes.chunks(chunk.max(1)) {
            items.extend(engine.advance(slice));
        }
        items
    }

    /// Screen swaps, resizes and block-closing markers are left out: each retires the ids it
    /// settles, so a later rewrite of those rows is a new line that no equality against the
    /// final grid can express.
    fn any_fragment() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            "[a-z ]{0,10}".prop_map(String::into_bytes),
            Just(b"\r\n".to_vec()),
            Just(b"\n".to_vec()),
            Just(b"\r".to_vec()),
            Just(b"\t".to_vec()),
            Just(b"\x08".to_vec()),
            Just(b"\x1b[31m".to_vec()),
            Just(b"\x1b[1;32m".to_vec()),
            Just(b"\x1b[44m".to_vec()),
            Just(b"\x1b[38;5;200m".to_vec()),
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

    fn assert_matches_the_grid(transcript: &[u8]) {
        let reference = without_blanks(reference_lines(transcript, 12, 4));
        for chunk in [1, 3, 7, 29, transcript.len()] {
            assert_eq!(
                without_blanks(replay(&drive(transcript, chunk))),
                reference,
                "chunk {chunk}"
            );
        }
    }

    #[test]
    fn a_line_split_by_erasing_its_wrap_keeps_its_place_above_the_next() {
        assert_matches_the_grid(b"aaa aaa  aaaa\x1b[A\r\n\r\nb\x1b[A\x1b[A\x1b[K");
    }

    #[test]
    fn a_line_written_above_an_earlier_one_keeps_its_place() {
        assert_matches_the_grid(
            b"\r\n\x1b[Ba\t\x1b[3X\x1b[A\x1b[31m\x1b[K\x1b[Aaaa b  \x1b[A\x1b[2K",
        );
    }

    #[test]
    fn a_wrapped_line_rewritten_above_a_later_one_keeps_its_place() {
        assert_matches_the_grid(b"\r\n\x1b[B a \x1b[Aa\x1b[Aaaaaa a  \x1b[A\x1b[K");
    }

    proptest! {
        #[test]
        fn never_panics_on_arbitrary_bytes(chunks in prop::collection::vec(any_bytes(), 0..4)) {
            let mut engine = AlacrittyEngine::with_staging_rows(12, 4, 64);
            for chunk in chunks {
                let _ = engine.advance(&chunk);
            }
            let _ = engine.take_replies();
            let _ = engine.screen();
        }

        #[test]
        fn no_line_is_ever_lost(transcript in any_transcript(), chunk in 1usize..40) {
            let items = drive(&transcript, chunk);

            prop_assert_eq!(
                without_blanks(replay(&items)),
                without_blanks(reference_lines(&transcript, 12, 4))
            );
        }

        #[test]
        fn every_colour_reaches_the_reader(transcript in any_transcript(), chunk in 1usize..40) {
            let items = drive(&transcript, chunk);

            prop_assert_eq!(
                without_blank_lines(replay_styled(&items)),
                without_blank_lines(reference_styled_lines(&transcript))
            );
        }

        #[test]
        fn reconstruction_is_independent_of_chunking(transcript in any_transcript()) {
            let whole = without_blanks(replay(&drive(&transcript, transcript.len().max(1))));
            for chunk in [1, 3, 7, 29] {
                let chunked = without_blanks(replay(&drive(&transcript, chunk)));
                prop_assert_eq!(chunked, whole.clone());
            }
        }

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

    mod colour {
        use super::*;

        const RED: Style = Style {
            fg: Some(Colour::Named { index: 1 }),
            bg: None,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            inverse: false,
            strike: false,
        };

        fn run(start: u32, len: u32, style: Style) -> StyleRun {
            StyleRun { start, len, style }
        }

        fn plain() -> Style {
            Style::default()
        }

        #[test]
        fn a_red_word_in_plain_text_is_one_run() {
            let items = engine().advance(b"an \x1b[31merror\x1b[0m here");

            assert_eq!(
                items,
                vec![styled(0, "an error here", APPENDED, vec![run(3, 5, RED)])]
            );
        }

        #[test]
        fn a_colour_change_in_the_middle_of_a_line_starts_a_new_run() {
            let items = engine().advance(b"\x1b[32mok\x1b[1;31mbad\x1b[0m.");

            let green = Style {
                fg: Some(Colour::Named { index: 2 }),
                ..plain()
            };
            let bold_red = Style { bold: true, ..RED };
            assert_eq!(
                items,
                vec![styled(
                    0,
                    "okbad.",
                    APPENDED,
                    vec![run(0, 2, green), run(2, 3, bold_red)]
                )]
            );
        }

        #[test]
        fn every_kind_of_colour_is_carried() {
            let items = engine()
                .advance(b"\x1b[91ma\x1b[38;5;208mb\x1b[38;2;1;2;3mc\x1b[0;48;5;4md\x1b[38;5;9me");

            let fg = |colour| Style {
                fg: Some(colour),
                ..plain()
            };
            let blue_behind = Style {
                bg: Some(Colour::Named { index: 4 }),
                ..plain()
            };
            assert_eq!(
                items,
                vec![styled(
                    0,
                    "abcde",
                    APPENDED,
                    vec![
                        run(0, 1, fg(Colour::Named { index: 9 })),
                        run(1, 1, fg(Colour::Indexed { index: 208 })),
                        run(2, 1, fg(Colour::Rgb { r: 1, g: 2, b: 3 })),
                        run(3, 1, blue_behind),
                        run(
                            4,
                            1,
                            Style {
                                fg: Some(Colour::Named { index: 9 }),
                                ..blue_behind
                            }
                        ),
                    ]
                )],
                "a 256-colour index below 16 is the palette colour it names"
            );
        }

        #[test]
        fn every_attribute_is_carried_and_hidden_text_is_sent_as_it_is() {
            let items = engine().advance(
                b"\x1b[2ma\x1b[0;3mb\x1b[0;4:3mc\x1b[0;7md\x1b[0;9me\x1b[0;1mf\x1b[0;8mg\x1b[0m",
            );

            assert_eq!(
                items,
                vec![styled(
                    0,
                    "abcdefg",
                    APPENDED,
                    vec![
                        run(
                            0,
                            1,
                            Style {
                                dim: true,
                                ..plain()
                            }
                        ),
                        run(
                            1,
                            1,
                            Style {
                                italic: true,
                                ..plain()
                            }
                        ),
                        run(
                            2,
                            1,
                            Style {
                                underline: true,
                                ..plain()
                            }
                        ),
                        run(
                            3,
                            1,
                            Style {
                                inverse: true,
                                ..plain()
                            }
                        ),
                        run(
                            4,
                            1,
                            Style {
                                strike: true,
                                ..plain()
                            }
                        ),
                        run(
                            5,
                            1,
                            Style {
                                bold: true,
                                ..plain()
                            }
                        ),
                    ]
                )]
            );
        }

        #[test]
        fn trailing_coloured_blanks_are_trimmed_with_the_text() {
            let items = engine().advance(b"ab\x1b[41m   ");

            assert_eq!(items, vec![line(0, "ab", APPENDED)]);
        }

        #[test]
        fn an_append_carries_runs_counted_from_its_own_start() {
            let mut engine = engine();

            let first = engine.advance(b"ab");
            let second = engine.advance(b"\x1b[31mcd");
            let third = engine.advance(b"e");

            assert_eq!(first, vec![line(0, "ab", APPENDED)]);
            assert_eq!(
                second,
                vec![styled(0, "cd", APPENDED, vec![run(0, 2, RED)])]
            );
            assert_eq!(third, vec![styled(0, "e", APPENDED, vec![run(0, 1, RED)])]);
        }

        #[test]
        fn a_colour_only_change_is_a_rewrite_of_the_same_text() {
            let mut engine = engine();
            let _ = engine.advance(b"abc");

            let items = engine.advance(b"\r\x1b[31mabc");

            assert_eq!(
                items,
                vec![styled(0, "abc", REWRITTEN, vec![run(0, 3, RED)])]
            );
        }

        #[test]
        fn a_recoloured_start_with_text_added_is_a_rewrite_of_the_whole_line() {
            let mut engine = engine();
            let _ = engine.advance(b"ab");

            let items = engine.advance(b"\r\x1b[31mabc");

            assert_eq!(
                items,
                vec![styled(0, "abc", REWRITTEN, vec![run(0, 3, RED)])]
            );
        }

        #[test]
        fn a_settled_line_carries_the_runs_of_the_whole_line() {
            let mut engine = engine();
            let _ = engine.advance(b"\x1b[31mab\x1b[0mc");
            let _ = engine.advance(b"d");

            let items = engine.advance(b"\x1b]133;D;0\x07");

            assert_eq!(
                items,
                vec![
                    styled(0, "abcd", SETTLED, vec![run(0, 2, RED)]),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(0)))),
                ]
            );
        }

        #[test]
        fn a_wide_character_counts_as_the_utf16_units_it_takes() {
            let items = engine()
                .advance("\u{4e2d}\x1b[31m\u{6587}\x1b[0m!\x1b[31m\u{1f600}\x1b[0mx".as_bytes());

            assert_eq!(
                items,
                vec![styled(
                    0,
                    "\u{4e2d}\u{6587}!\u{1f600}x",
                    APPENDED,
                    vec![run(1, 1, RED), run(3, 2, RED)]
                )]
            );
        }

        #[test]
        fn a_zero_width_character_belongs_to_its_base_characters_run() {
            let items = engine().advance("\x1b[31me\x1b[0m\u{301}x".as_bytes());

            assert_eq!(
                items,
                vec![styled(0, "e\u{301}x", APPENDED, vec![run(0, 2, RED)])]
            );
        }
    }
}
