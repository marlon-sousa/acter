//! Policy: the command-block boundary tracker — DESIGN's linchpin for non-interactive
//! mode. Takes the ordered stream of identified lines, OSC 133 markers and screen
//! transitions the terminal engine emits, and says where each command block begins and
//! ends and which region every line fell in.
//!
//! It cuts; it never extracts and never filters (spec B2, decision 2). Items pass
//! through unchanged except for the region label: same id, same text, same revision, in
//! the same order. Nothing is dropped, nothing is rewritten, and no opinion is formed
//! about what a region is *for*: DESIGN's echo exclusion — block content is C..D only —
//! is then a caller's filter over labelled regions rather than a rule buried in a state
//! machine.
//!
//! Pure: no clock, no ports, no identity of its own. Line identity is minted by the
//! engine and only carried here; command ids and the integration grace period belong to
//! the service above it (decision 5), because both need state this layer deliberately
//! does not have.

use crate::{ExitCode, LineId, LineRevision, Osc133Marker, Screen, ShellMarkers, TerminalItem};

/// Where a piece of text fell relative to the markers around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Region {
    /// No block context at all: before the first marker, in a session whose integration
    /// never appeared (DESIGN's reliability case 2), or between a command's end and the
    /// next prompt. Not an error path — such text is still rendered, it is only never
    /// treated as a command's output. A terminal that silently drops text is worse than
    /// one that admits it does not know where the text belongs.
    #[default]
    Unstructured,
    /// A..B — the prompt the shell drew.
    Prompt,
    /// B..C — the shell's echo of the submitted line. Excluded from block content: the
    /// frontend already renders the submitted command from its own edit field.
    CommandLine,
    /// C..D — the command's output. The only region that is block content.
    Output,
}

/// What the tracker concluded about one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryEvent {
    /// The first marker of the session arrived: shell integration is present. Maps onto
    /// [`SessionState::markers_observed`](crate::SessionState::markers_observed), and
    /// fires exactly once — see [`BoundaryTracker`] for why once is enough.
    MarkersObserved,
    /// A command's output region opened.
    BlockStarted,
    /// One line item, labelled with the region it fell in. Everything else about it —
    /// which line, what text, what the text did to that line — is carried through
    /// untouched.
    Line {
        region: Region,
        id: LineId,
        text: String,
        revision: LineRevision,
    },
    /// The open block closed. `exit` is `None` when the end was not a well-formed `D`
    /// carrying a code — either a bare `D`, or a prompt reappearing mid-block.
    BlockEnded { exit: Option<ExitCode> },
    /// The emulator switched screens, passed through at its place in the stream.
    ///
    /// It travels here rather than on a side channel so ordering stays decided in one
    /// place: the alternate screen is entered in the middle of a batch, and which lines
    /// arrived before the switch is exactly what a caller must not have to reconstruct.
    ScreenChanged(Screen),
}

/// One session's boundary state machine.
///
/// Its state is the current region, a latch, whether a block is open, and — for a shell
/// that marks only `A` and `B` — the row the command line is being echoed onto.
///
/// **Openness used to be exactly `region == Output`, and B4.5 is what separated them.** In
/// a shell that emits no `D` the block closes on `B`, so the returning prompt falls inside
/// the block of the command it ended (decision 3) — which means the region is `Prompt`
/// while a block is still open, and the two facts stop being the same fact. For a
/// [`ShellMarkers::Full`] shell they remain equivalent, and every path below behaves as it
/// did.
///
/// The latch fires once and that is sufficient, including for DESIGN decision 8's
/// recovery case: `SessionState` only ever moves *into* `Integrated`, and
/// `grace_period_expired` only resolves `Pending`, so if the grace period expired first
/// the latch has not fired yet and the first marker to arrive still recovers the session.
#[derive(Debug, Default)]
pub struct BoundaryTracker {
    region: Region,
    markers_seen: bool,
    /// What the far end's prompt is able to say. A [`ShellMarkers::Full`] shell drives
    /// every path in this type exactly as it did before B4.5; the two extra rules below
    /// are reachable only for a shell that declares it emits no `C` (spec B4.5).
    markers: ShellMarkers,
    /// The row the prompt was last drawn on. It is the row the echo of the submitted line
    /// is written onto — measured against a real `cmd.exe` — so it is what a synthesized
    /// `C` is positioned against.
    prompt_row: Option<LineId>,
    /// The row the command line is being echoed onto, while one is. `None` outside
    /// `B..C`, and `None` inside it until the first row of the region is known.
    command_line_row: Option<LineId>,
    /// Whether a block is open. See the type's doc: not derivable from the region any more.
    block_open: bool,
}

impl BoundaryTracker {
    /// A tracker for a far end whose prompt says this much.
    ///
    /// The shell is named at construction rather than discovered, because it is a
    /// declaration: what a shell *can* emit is knowledge the adapter that spawned it has,
    /// and inferring it from what has arrived so far would make every early command behave
    /// differently from every later one.
    pub fn new(markers: ShellMarkers) -> Self {
        Self {
            markers,
            ..Self::default()
        }
    }

    /// Where the far end is writing right now.
    ///
    /// Exposed for one caller and one question: whether the shell has drawn its prompt and
    /// is reading a line, which is `Prompt` or `CommandLine` and nothing else. The service
    /// needs it to decide whether a byte meant for a line editor may safely be sent (spec
    /// B4.5, decision 7), and it is a fact this type already holds rather than one anybody
    /// should reconstruct from the event stream.
    pub fn region(&self) -> Region {
        self.region
    }

    /// Observes a batch of items and returns what it concluded.
    ///
    /// Batch in, batch out, because that is the shape the caller has: one batch of items
    /// per read from the transport (decision 9). State carries across calls — a marker in
    /// one batch governs the lines in the next.
    pub fn observe(&mut self, items: impl IntoIterator<Item = TerminalItem>) -> Vec<BoundaryEvent> {
        let mut events = Vec::new();
        for item in items {
            match item {
                // Empty text passes through rather than being swallowed: B1.1 made an
                // empty chunk meaningful — it must not move the quiescence deadline — so
                // dropping one here would hide the case the pacing policy was built for.
                TerminalItem::Line { id, text, revision } => {
                    self.observe_row(id, &mut events);
                    events.push(BoundaryEvent::Line {
                        region: self.region,
                        id,
                        text,
                        revision,
                    });
                }
                // A marker is one of the things that can end a command-line region in a
                // shell that marks no `C`: the next prompt's `A` is what arrives when a
                // command produced no output at all.
                TerminalItem::Marker(marker) => {
                    self.end_command_line(&mut events);
                    self.observe_marker(marker, &mut events);
                }
                // A screen change says nothing about command blocks: a program redrawing
                // on the alternate screen has neither started nor finished a command, so
                // the region is left exactly as it was and the item is only relayed.
                TerminalItem::ScreenChanged(screen) => {
                    events.push(BoundaryEvent::ScreenChanged(screen))
                }
            }
        }
        events
    }

    /// Which row this line item is on, and whether that ends the command line.
    ///
    /// Only two things are recorded and only one decision is taken, both of them dead
    /// weight for a [`ShellMarkers::Full`] shell. The prompt's row is remembered because
    /// it is the row the echo will be written onto; and once the command line is being
    /// echoed, **anything that is not a further append to that row ends the region**,
    /// which is where a `C` nobody sent is synthesized (spec B4.5, decision 2).
    ///
    /// That single rule is what makes the region safe to exclude from a block's content.
    /// Real output arriving while the region is open lands on a *new* row, ends the region
    /// and is labelled `Output`; what stays behind is the echo itself and anything the far
    /// end appended to the same row, which is the one thing a caller is entitled to drop.
    fn observe_row(&mut self, id: LineId, events: &mut Vec<BoundaryEvent>) {
        if self.region == Region::Prompt {
            self.prompt_row = Some(id);
            return;
        }
        if self.markers.marks_output_start() || self.region != Region::CommandLine {
            return;
        }
        match self.command_line_row {
            // No prompt was marked before this `B`, so the first row of the region is the
            // row: refusing it would only lose the boundary.
            None => self.command_line_row = Some(id),
            Some(row) if row == id => {}
            Some(_) => self.end_command_line(events),
        }
    }

    /// Ends a command-line region by supplying the `C` the shell cannot.
    ///
    /// A no-op for a shell that marks its own output start, and a no-op outside `B..C`, so
    /// callers do not have to ask either question before calling.
    fn end_command_line(&mut self, events: &mut Vec<BoundaryEvent>) {
        if self.markers.marks_output_start() || self.region != Region::CommandLine {
            return;
        }
        self.command_line_row = None;
        self.region = Region::Output;
        self.block_open = true;
        events.push(BoundaryEvent::BlockStarted);
    }

    fn observe_marker(&mut self, marker: Osc133Marker, events: &mut Vec<BoundaryEvent>) {
        if !self.markers_seen {
            self.markers_seen = true;
            events.push(BoundaryEvent::MarkersObserved);
        }

        match marker {
            // A and B both mean the shell is back at its prompt, and a prompt cannot
            // reappear before D — which is exactly what makes D deterministic. Arriving
            // mid-block, either one means the integration lied or a program forged a
            // marker; closing keeps the session speakable, where ignoring would strand it
            // in "running" until it is torn down (decision 6).
            //
            // **Except in a shell that emits no `D`**, where the block closes on `B`
            // instead (spec B4.5, decision 3). There the returning prompt is the only
            // ending a listener gets — no exit code exists to announce — so it has to fall
            // inside the block of the command that just ended rather than after it. `A`,
            // the prompt text and `B` are one write in `cmd.exe`, so the close is a few
            // bytes later either way.
            Osc133Marker::PromptStart => {
                if self.markers.marks_output_start() {
                    self.end_open_block(None, events);
                }
                self.region = Region::Prompt;
                // Forgotten rather than carried: a `B` with no prompt text behind it must
                // leave the first row of the command line to claim the region, not adopt
                // whichever row the *previous* prompt happened to use.
                self.prompt_row = None;
            }
            // Accepted with or without a preceding A: it means the command line begins
            // here, and refusing it would only lose text (decision 7).
            Osc133Marker::CommandStart => {
                self.end_open_block(None, events);
                self.region = Region::CommandLine;
                // The row the prompt was drawn on is the row the echo is written onto;
                // `None` when no prompt was marked, which leaves the first row of the
                // region to claim it.
                self.command_line_row = self.prompt_row;
            }
            // A second C does not split the block: a program redrawing has not restarted,
            // the same reasoning `SessionState` applies to alt-screen entry.
            Osc133Marker::OutputStart => {
                self.command_line_row = None;
                self.region = Region::Output;
                if !self.block_open {
                    self.block_open = true;
                    events.push(BoundaryEvent::BlockStarted);
                }
            }
            // D with no open block is ignored outright, region untouched (DESIGN names
            // this case).
            Osc133Marker::CommandEnd(exit) => {
                if self.block_open {
                    self.block_open = false;
                    events.push(BoundaryEvent::BlockEnded { exit });
                    self.region = Region::Unstructured;
                }
            }
        }
    }

    /// Emits the close for an open block, if one is open. The caller sets the new region.
    fn end_open_block(&mut self, exit: Option<ExitCode>, events: &mut Vec<BoundaryEvent>) {
        if self.block_open {
            self.block_open = false;
            events.push(BoundaryEvent::BlockEnded { exit });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use proptest::prelude::*;

    use super::*;

    /// A counter for tests that do not care which id they get, only that one is carried.
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    /// Most table tests below care about labelling rather than revision, so this helper
    /// mints a fresh id and appends: the ordinary shape of streaming output.
    fn text(s: &str) -> TerminalItem {
        line(
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
            s,
            LineRevision::Appended,
        )
    }

    fn line(id: u64, s: &str, revision: LineRevision) -> TerminalItem {
        TerminalItem::Line {
            id: LineId(id),
            text: s.to_owned(),
            revision,
        }
    }

    fn a() -> TerminalItem {
        TerminalItem::Marker(Osc133Marker::PromptStart)
    }

    fn b() -> TerminalItem {
        TerminalItem::Marker(Osc133Marker::CommandStart)
    }

    fn c() -> TerminalItem {
        TerminalItem::Marker(Osc133Marker::OutputStart)
    }

    fn d(code: i32) -> TerminalItem {
        TerminalItem::Marker(Osc133Marker::CommandEnd(Some(ExitCode(code))))
    }

    fn bare_d() -> TerminalItem {
        TerminalItem::Marker(Osc133Marker::CommandEnd(None))
    }

    fn regions(events: &[BoundaryEvent]) -> Vec<Region> {
        events
            .iter()
            .filter_map(|event| match event {
                BoundaryEvent::Line { region, .. } => Some(*region),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn happy_cycle_brackets_the_block_and_labels_its_line() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events =
            tracker.observe([a(), b(), c(), line(7, "hello", LineRevision::Settled), d(0)]);

        assert_eq!(
            events,
            vec![
                BoundaryEvent::MarkersObserved,
                BoundaryEvent::BlockStarted,
                BoundaryEvent::Line {
                    region: Region::Output,
                    id: LineId(7),
                    text: "hello".to_owned(),
                    revision: LineRevision::Settled,
                },
                BoundaryEvent::BlockEnded {
                    exit: Some(ExitCode(0)),
                },
            ]
        );
    }

    #[test]
    fn echo_exclusion_labels_prompt_and_command_line_apart_from_output() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([
            a(),
            text("PS C:\\> "),
            b(),
            text("git status"),
            c(),
            text("nothing to commit"),
            d(0),
        ]);

        assert_eq!(
            regions(&events),
            vec![Region::Prompt, Region::CommandLine, Region::Output]
        );
    }

    #[test]
    fn text_before_any_marker_is_unstructured() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([text("banner from the shell")]);

        assert_eq!(regions(&events), vec![Region::Unstructured]);
        assert!(!events.contains(&BoundaryEvent::MarkersObserved));
    }

    #[test]
    fn text_between_a_command_end_and_the_next_prompt_is_unstructured() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([c(), d(0), text("stray")]);

        assert_eq!(regions(&events), vec![Region::Unstructured]);
    }

    #[test]
    fn command_end_with_no_open_block_is_ignored() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([
            a(),
            d(0),
            line(3, "still the prompt", LineRevision::Appended),
        ]);

        assert_eq!(
            events,
            vec![
                BoundaryEvent::MarkersObserved,
                BoundaryEvent::Line {
                    region: Region::Prompt,
                    id: LineId(3),
                    text: "still the prompt".to_owned(),
                    revision: LineRevision::Appended,
                },
            ]
        );
    }

    #[test]
    fn a_second_output_start_does_not_split_the_block() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([c(), text("one"), c(), text("two"), d(0)]);

        let starts = events
            .iter()
            .filter(|event| **event == BoundaryEvent::BlockStarted)
            .count();
        assert_eq!(starts, 1);
        assert_eq!(regions(&events), vec![Region::Output, Region::Output]);
    }

    #[test]
    fn prompt_start_mid_block_closes_it_with_an_unknown_exit_code() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([
            c(),
            line(1, "output", LineRevision::Appended),
            a(),
            line(2, "prompt", LineRevision::Appended),
        ]);

        assert_eq!(
            events,
            vec![
                BoundaryEvent::MarkersObserved,
                BoundaryEvent::BlockStarted,
                BoundaryEvent::Line {
                    region: Region::Output,
                    id: LineId(1),
                    text: "output".to_owned(),
                    revision: LineRevision::Appended,
                },
                BoundaryEvent::BlockEnded { exit: None },
                BoundaryEvent::Line {
                    region: Region::Prompt,
                    id: LineId(2),
                    text: "prompt".to_owned(),
                    revision: LineRevision::Appended,
                },
            ]
        );
    }

    #[test]
    fn command_start_mid_block_closes_it_with_an_unknown_exit_code() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([c(), b()]);

        assert!(events.contains(&BoundaryEvent::BlockEnded { exit: None }));
    }

    #[test]
    fn a_bare_command_end_closes_the_block_with_no_exit_code() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([c(), bare_d()]);

        assert!(events.contains(&BoundaryEvent::BlockEnded { exit: None }));
    }

    #[test]
    fn command_start_without_a_preceding_prompt_start_is_accepted() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([b(), text("git status")]);

        assert_eq!(regions(&events), vec![Region::CommandLine]);
    }

    #[test]
    fn markers_observed_fires_once_across_two_commands() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([a(), b(), c(), d(0), a(), b(), c(), d(1)]);

        let observed = events
            .iter()
            .filter(|event| **event == BoundaryEvent::MarkersObserved)
            .count();
        assert_eq!(observed, 1);
    }

    #[test]
    fn empty_text_passes_through_rather_than_being_swallowed() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([c(), line(9, "", LineRevision::Appended)]);

        assert!(events.contains(&BoundaryEvent::Line {
            region: Region::Output,
            id: LineId(9),
            text: String::new(),
            revision: LineRevision::Appended,
        }));
    }

    #[test]
    fn every_revision_kind_survives_the_label() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([
            c(),
            line(4, "downloading", LineRevision::Appended),
            line(4, "done", LineRevision::Rewritten),
            line(4, "done", LineRevision::Settled),
        ]);

        let revisions: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                BoundaryEvent::Line { revision, .. } => Some(*revision),
                _ => None,
            })
            .collect();
        assert_eq!(
            revisions,
            vec![
                LineRevision::Appended,
                LineRevision::Rewritten,
                LineRevision::Settled,
            ]
        );
    }

    #[test]
    fn a_screen_change_is_relayed_in_place_without_touching_the_region() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let events = tracker.observe([
            c(),
            line(1, "before", LineRevision::Appended),
            TerminalItem::ScreenChanged(Screen::Alternate),
            line(2, "after", LineRevision::Appended),
        ]);

        assert_eq!(
            events,
            vec![
                BoundaryEvent::MarkersObserved,
                BoundaryEvent::BlockStarted,
                BoundaryEvent::Line {
                    region: Region::Output,
                    id: LineId(1),
                    text: "before".to_owned(),
                    revision: LineRevision::Appended,
                },
                BoundaryEvent::ScreenChanged(Screen::Alternate),
                BoundaryEvent::Line {
                    region: Region::Output,
                    id: LineId(2),
                    text: "after".to_owned(),
                    revision: LineRevision::Appended,
                },
            ]
        );
    }

    #[test]
    fn state_carries_across_batches() {
        let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
        let first = tracker.observe([c()]);
        let second = tracker.observe([text("output arriving in the next read")]);

        assert_eq!(
            first,
            vec![BoundaryEvent::MarkersObserved, BoundaryEvent::BlockStarted]
        );
        assert_eq!(regions(&second), vec![Region::Output]);
    }

    fn any_revision() -> impl Strategy<Value = LineRevision> {
        prop_oneof![
            Just(LineRevision::Appended),
            Just(LineRevision::Rewritten),
            Just(LineRevision::Settled),
        ]
    }

    fn any_item() -> impl Strategy<Value = TerminalItem> {
        prop_oneof![
            (any::<u64>(), any::<String>(), any_revision()).prop_map(|(id, text, revision)| {
                TerminalItem::Line {
                    id: LineId(id),
                    text,
                    revision,
                }
            }),
            Just(TerminalItem::Marker(Osc133Marker::PromptStart)),
            Just(TerminalItem::Marker(Osc133Marker::CommandStart)),
            Just(TerminalItem::Marker(Osc133Marker::OutputStart)),
            any::<Option<i32>>().prop_map(|code| TerminalItem::Marker(Osc133Marker::CommandEnd(
                code.map(ExitCode)
            ))),
            Just(TerminalItem::ScreenChanged(Screen::Normal)),
            Just(TerminalItem::ScreenChanged(Screen::Alternate)),
        ]
    }

    fn any_stream() -> impl Strategy<Value = Vec<TerminalItem>> {
        prop::collection::vec(any_item(), 0..64)
    }

    proptest! {
        /// The property that matters most for this product: for a screen-reader terminal,
        /// silently dropping output is the cardinal defect. Every line item comes out, in
        /// order, carrying the same id, the same text and the same revision it went in
        /// with — the tracker adds a label and nothing else. Screen changes are relayed
        /// on the same terms.
        ///
        /// This is B2's "text is never lost" restated for identified lines. Identity made
        /// the old concatenation equality ill-typed, and the replacement is strictly
        /// stronger: it no longer has to reason about what the text means.
        #[test]
        fn items_pass_through_unchanged_except_for_the_region_label(items in any_stream()) {
            let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
            let events = tracker.observe(items.clone());

            let given: Vec<_> = items
                .iter()
                .filter(|item| !matches!(item, TerminalItem::Marker(_)))
                .cloned()
                .collect();
            let emitted: Vec<_> = events
                .iter()
                .filter_map(|event| match event {
                    BoundaryEvent::Line { id, text, revision, .. } => Some(TerminalItem::Line {
                        id: *id,
                        text: text.clone(),
                        revision: *revision,
                    }),
                    BoundaryEvent::ScreenChanged(screen) => {
                        Some(TerminalItem::ScreenChanged(*screen))
                    }
                    _ => None,
                })
                .collect();

            prop_assert_eq!(given, emitted);
        }

        #[test]
        fn blocks_are_balanced_and_never_nest(items in any_stream()) {
            let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
            let events = tracker.observe(items);

            let mut open = false;
            for event in &events {
                match event {
                    BoundaryEvent::BlockStarted => {
                        prop_assert!(!open, "a block started while one was already open");
                        open = true;
                    }
                    BoundaryEvent::BlockEnded { .. } => {
                        prop_assert!(open, "a block ended with none open");
                        open = false;
                    }
                    _ => {}
                }
            }
        }

        #[test]
        fn markers_observed_fires_at_most_once_and_only_when_a_marker_arrived(
            items in any_stream()
        ) {
            let mut tracker = BoundaryTracker::new(ShellMarkers::Full);
            let had_marker = items
                .iter()
                .any(|item| matches!(item, TerminalItem::Marker(_)));
            let events = tracker.observe(items);

            let observed = events
                .iter()
                .filter(|event| **event == BoundaryEvent::MarkersObserved)
                .count();
            prop_assert_eq!(observed, usize::from(had_marker));
        }
    }

    /// A shell whose prompt marks only `A` and `B` — `cmd.exe` (spec B4.5). Every test
    /// below drives the sequence a real one puts on the wire: `A`, the prompt text, `B`
    /// all in one write, then the echo appended to the prompt's own row.
    mod a_shell_that_marks_no_output_start {
        use super::*;

        fn cmd() -> BoundaryTracker {
            BoundaryTracker::new(ShellMarkers::PromptAndCommandLine)
        }

        /// The rule itself: the echoed line is `B..C`, and the block opens where that row
        /// stops being appended to.
        #[test]
        fn the_block_opens_where_the_echoed_line_ends() {
            let mut tracker = cmd();
            let events = tracker.observe([
                a(),
                line(1, r"C:\>", LineRevision::Appended),
                b(),
                line(1, "dir", LineRevision::Appended),
                line(2, "one.txt", LineRevision::Appended),
            ]);

            assert_eq!(
                regions(&events),
                vec![Region::Prompt, Region::CommandLine, Region::Output]
            );
            assert!(events.contains(&BoundaryEvent::BlockStarted));
        }

        /// A command line wide enough to wrap: `cmd.exe` swallows the continuation into
        /// the same row, so the region must not end on it.
        #[test]
        fn a_wrapped_command_line_does_not_open_the_block_early() {
            let mut tracker = cmd();
            let events = tracker.observe([
                a(),
                line(1, r"C:\>", LineRevision::Appended),
                b(),
                line(1, "a-very-long-", LineRevision::Appended),
                line(1, "command-line", LineRevision::Appended),
            ]);

            assert_eq!(
                regions(&events),
                vec![Region::Prompt, Region::CommandLine, Region::CommandLine]
            );
            assert!(!events.contains(&BoundaryEvent::BlockStarted));
        }

        /// A command that printed nothing at all. The next prompt's `A` is what arrives,
        /// and it has to end the region — otherwise the block never opens and the
        /// submission reports running forever.
        #[test]
        fn a_command_with_no_output_still_opens_and_closes_a_block() {
            let mut tracker = cmd();
            let events = tracker.observe([
                a(),
                line(1, r"C:\>", LineRevision::Appended),
                b(),
                line(1, "cd docs", LineRevision::Appended),
                a(),
                line(2, r"C:\docs>", LineRevision::Appended),
                b(),
            ]);

            let structure: Vec<_> = events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        BoundaryEvent::BlockStarted | BoundaryEvent::BlockEnded { .. }
                    )
                })
                .collect();
            assert_eq!(
                structure,
                vec![
                    &BoundaryEvent::BlockStarted,
                    &BoundaryEvent::BlockEnded { exit: None },
                ]
            );
        }

        /// Decision 3: the block closes on `B`, not on `A`, so the returning prompt is
        /// inside the block of the command that just ended. It is the only ending a
        /// listener gets in a shell with no exit code.
        #[test]
        fn the_returning_prompt_is_the_last_thing_in_the_block_it_ends() {
            let mut tracker = cmd();
            tracker.observe([a(), line(1, r"C:\>", LineRevision::Appended), b()]);
            let events = tracker.observe([
                line(1, "dir", LineRevision::Appended),
                line(2, "one.txt", LineRevision::Appended),
                a(),
                line(3, r"C:\>", LineRevision::Appended),
                b(),
            ]);

            let labelled: Vec<_> = events
                .iter()
                .filter_map(|event| match event {
                    BoundaryEvent::Line { region, text, .. } => Some((*region, text.as_str())),
                    _ => None,
                })
                .collect();
            assert_eq!(
                labelled,
                vec![
                    (Region::CommandLine, "dir"),
                    (Region::Output, "one.txt"),
                    // Labelled `Prompt` and still inside the open block, which is the
                    // whole of decisions 3 and 4: the tracker says where the text was, and
                    // `Pump::wants` is what makes a prompt content in a shell with no `D`.
                    (Region::Prompt, r"C:\>"),
                ]
            );
            let ended = events
                .iter()
                .position(|event| matches!(event, BoundaryEvent::BlockEnded { .. }))
                .expect("the block ends");
            let prompt = events
                .iter()
                .position(
                    |event| matches!(event, BoundaryEvent::Line { text, .. } if text == r"C:\>"),
                )
                .expect("the prompt is labelled");
            assert!(
                prompt < ended,
                "the prompt is spoken before the block closes"
            );
        }

        /// The safety constraint, which is the reason the rule keys on the row: text that
        /// cannot be an echo must be forwarded, not dropped. A new row while the command
        /// line is open ends the region and is labelled `Output`.
        #[test]
        fn text_on_a_new_row_is_output_and_never_stays_in_the_command_line() {
            let mut tracker = cmd();
            let events = tracker.observe([
                a(),
                line(1, r"C:\>", LineRevision::Appended),
                b(),
                line(9, "something the far end wrote", LineRevision::Appended),
            ]);

            assert_eq!(regions(&events), vec![Region::Prompt, Region::Output]);
        }

        /// A `B` with no prompt text behind it leaves the first row of the region to claim
        /// it, rather than adopting whichever row the previous prompt used.
        #[test]
        fn a_command_line_with_no_prompt_behind_it_claims_its_own_row() {
            let mut tracker = cmd();
            tracker.observe([a(), line(1, r"C:\>", LineRevision::Appended), b()]);
            let events = tracker.observe([
                a(),
                b(),
                line(4, "the echo", LineRevision::Appended),
                line(4, " continued", LineRevision::Appended),
            ]);

            assert_eq!(
                regions(&events),
                vec![Region::CommandLine, Region::CommandLine]
            );
        }

        /// Nothing about a `Full` shell changes, which is what keeps every session that
        /// came before B4.5 exactly where it was.
        #[test]
        fn the_full_cycle_is_untouched_by_the_declaration() {
            let mut full = BoundaryTracker::new(ShellMarkers::Full);
            let events = full.observe([
                a(),
                line(1, r"C:\>", LineRevision::Appended),
                b(),
                line(1, "dir", LineRevision::Appended),
                line(2, "one.txt", LineRevision::Appended),
            ]);

            // No `C`, so no block, and the second row is still the command line.
            assert!(!events.contains(&BoundaryEvent::BlockStarted));
            assert_eq!(
                regions(&events),
                vec![Region::Prompt, Region::CommandLine, Region::CommandLine]
            );
        }
    }
}
