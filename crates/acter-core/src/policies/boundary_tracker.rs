//! Policy: the command-block boundary tracker — DESIGN's linchpin for non-interactive
//! mode. Takes the ordered stream of text and OSC 133 markers the terminal engine emits,
//! and says where each command block begins and ends and which region every piece of
//! text fell in.
//!
//! It cuts; it never extracts and never filters (spec B2, decision 2). Nothing is
//! dropped, nothing is rewritten, and no opinion is formed about what a region is *for*:
//! DESIGN's echo exclusion — block content is C..D only — is then a caller's filter over
//! labelled regions rather than a rule buried in a state machine.
//!
//! Pure: no clock, no ports, no identity. Command ids and the integration grace period
//! belong to the service above it (decision 5), because both need state this layer
//! deliberately does not have.

use crate::{ExitCode, Osc133Marker, TerminalItem};

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
    /// Text, labelled with the region it fell in.
    Text { region: Region, text: String },
    /// The open block closed. `exit` is `None` when the end was not a well-formed `D`
    /// carrying a code — either a bare `D`, or a prompt reappearing mid-block.
    BlockEnded { exit: Option<ExitCode> },
}

/// One session's boundary state machine.
///
/// Its entire state is the current region and a latch. A block being open is exactly
/// `region == Output`, so that is not stored twice.
///
/// The latch fires once and that is sufficient, including for DESIGN decision 8's
/// recovery case: `SessionState` only ever moves *into* `Integrated`, and
/// `grace_period_expired` only resolves `Pending`, so if the grace period expired first
/// the latch has not fired yet and the first marker to arrive still recovers the session.
#[derive(Debug, Default)]
pub struct BoundaryTracker {
    region: Region,
    markers_seen: bool,
}

impl BoundaryTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Observes a batch of items and returns what it concluded.
    ///
    /// Batch in, batch out, because that is the shape the caller has: one batch of items
    /// per read from the transport (decision 9). State carries across calls — a marker in
    /// one batch governs the text in the next.
    pub fn observe(&mut self, items: impl IntoIterator<Item = TerminalItem>) -> Vec<BoundaryEvent> {
        let mut events = Vec::new();
        for item in items {
            match item {
                // Empty text passes through rather than being swallowed: B1.1 made an
                // empty chunk meaningful — it must not move the quiescence deadline — so
                // dropping one here would hide the case the pacing policy was built for.
                TerminalItem::Text(text) => events.push(BoundaryEvent::Text {
                    region: self.region,
                    text,
                }),
                TerminalItem::Marker(marker) => self.observe_marker(marker, &mut events),
            }
        }
        events
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
            Osc133Marker::PromptStart => {
                self.end_open_block(None, events);
                self.region = Region::Prompt;
            }
            // Accepted with or without a preceding A: it means the command line begins
            // here, and refusing it would only lose text (decision 7).
            Osc133Marker::CommandStart => {
                self.end_open_block(None, events);
                self.region = Region::CommandLine;
            }
            // A second C does not split the block: a program redrawing has not restarted,
            // the same reasoning `SessionState` applies to alt-screen entry.
            Osc133Marker::OutputStart => {
                if self.region != Region::Output {
                    self.region = Region::Output;
                    events.push(BoundaryEvent::BlockStarted);
                }
            }
            // D with no open block is ignored outright, region untouched (DESIGN names
            // this case).
            Osc133Marker::CommandEnd(exit) => {
                if self.region == Region::Output {
                    events.push(BoundaryEvent::BlockEnded { exit });
                    self.region = Region::Unstructured;
                }
            }
        }
    }

    /// Emits the close for an open block, if one is open. The caller sets the new region.
    fn end_open_block(&self, exit: Option<ExitCode>, events: &mut Vec<BoundaryEvent>) {
        if self.region == Region::Output {
            events.push(BoundaryEvent::BlockEnded { exit });
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn text(s: &str) -> TerminalItem {
        TerminalItem::Text(s.to_owned())
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
                BoundaryEvent::Text { region, .. } => Some(*region),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn happy_cycle_brackets_the_block_and_labels_its_text() {
        let mut tracker = BoundaryTracker::new();
        let events = tracker.observe([a(), b(), c(), text("hello\n"), d(0)]);

        assert_eq!(
            events,
            vec![
                BoundaryEvent::MarkersObserved,
                BoundaryEvent::BlockStarted,
                BoundaryEvent::Text {
                    region: Region::Output,
                    text: "hello\n".to_owned(),
                },
                BoundaryEvent::BlockEnded {
                    exit: Some(ExitCode(0)),
                },
            ]
        );
    }

    #[test]
    fn echo_exclusion_labels_prompt_and_command_line_apart_from_output() {
        let mut tracker = BoundaryTracker::new();
        let events = tracker.observe([
            a(),
            text("PS C:\\> "),
            b(),
            text("git status\r\n"),
            c(),
            text("nothing to commit\n"),
            d(0),
        ]);

        assert_eq!(
            regions(&events),
            vec![Region::Prompt, Region::CommandLine, Region::Output]
        );
    }

    #[test]
    fn text_before_any_marker_is_unstructured() {
        let mut tracker = BoundaryTracker::new();
        let events = tracker.observe([text("banner from the shell\n")]);

        assert_eq!(regions(&events), vec![Region::Unstructured]);
        assert!(!events.contains(&BoundaryEvent::MarkersObserved));
    }

    #[test]
    fn text_between_a_command_end_and_the_next_prompt_is_unstructured() {
        let mut tracker = BoundaryTracker::new();
        let events = tracker.observe([c(), d(0), text("stray\n")]);

        assert_eq!(regions(&events), vec![Region::Unstructured]);
    }

    #[test]
    fn command_end_with_no_open_block_is_ignored() {
        let mut tracker = BoundaryTracker::new();
        let events = tracker.observe([a(), d(0), text("still the prompt")]);

        assert_eq!(
            events,
            vec![
                BoundaryEvent::MarkersObserved,
                BoundaryEvent::Text {
                    region: Region::Prompt,
                    text: "still the prompt".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_second_output_start_does_not_split_the_block() {
        let mut tracker = BoundaryTracker::new();
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
        let mut tracker = BoundaryTracker::new();
        let events = tracker.observe([c(), text("output"), a(), text("prompt")]);

        assert_eq!(
            events,
            vec![
                BoundaryEvent::MarkersObserved,
                BoundaryEvent::BlockStarted,
                BoundaryEvent::Text {
                    region: Region::Output,
                    text: "output".to_owned(),
                },
                BoundaryEvent::BlockEnded { exit: None },
                BoundaryEvent::Text {
                    region: Region::Prompt,
                    text: "prompt".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn command_start_mid_block_closes_it_with_an_unknown_exit_code() {
        let mut tracker = BoundaryTracker::new();
        let events = tracker.observe([c(), b()]);

        assert!(events.contains(&BoundaryEvent::BlockEnded { exit: None }));
    }

    #[test]
    fn a_bare_command_end_closes_the_block_with_no_exit_code() {
        let mut tracker = BoundaryTracker::new();
        let events = tracker.observe([c(), bare_d()]);

        assert!(events.contains(&BoundaryEvent::BlockEnded { exit: None }));
    }

    #[test]
    fn command_start_without_a_preceding_prompt_start_is_accepted() {
        let mut tracker = BoundaryTracker::new();
        let events = tracker.observe([b(), text("git status")]);

        assert_eq!(regions(&events), vec![Region::CommandLine]);
    }

    #[test]
    fn markers_observed_fires_once_across_two_commands() {
        let mut tracker = BoundaryTracker::new();
        let events = tracker.observe([a(), b(), c(), d(0), a(), b(), c(), d(1)]);

        let observed = events
            .iter()
            .filter(|event| **event == BoundaryEvent::MarkersObserved)
            .count();
        assert_eq!(observed, 1);
    }

    #[test]
    fn empty_text_passes_through_rather_than_being_swallowed() {
        let mut tracker = BoundaryTracker::new();
        let events = tracker.observe([c(), text("")]);

        assert!(events.contains(&BoundaryEvent::Text {
            region: Region::Output,
            text: String::new(),
        }));
    }

    #[test]
    fn state_carries_across_batches() {
        let mut tracker = BoundaryTracker::new();
        let first = tracker.observe([c()]);
        let second = tracker.observe([text("output arriving in the next read")]);

        assert_eq!(
            first,
            vec![BoundaryEvent::MarkersObserved, BoundaryEvent::BlockStarted]
        );
        assert_eq!(regions(&second), vec![Region::Output]);
    }

    fn any_item() -> impl Strategy<Value = TerminalItem> {
        prop_oneof![
            any::<String>().prop_map(TerminalItem::Text),
            Just(TerminalItem::Marker(Osc133Marker::PromptStart)),
            Just(TerminalItem::Marker(Osc133Marker::CommandStart)),
            Just(TerminalItem::Marker(Osc133Marker::OutputStart)),
            any::<Option<i32>>().prop_map(|code| TerminalItem::Marker(Osc133Marker::CommandEnd(
                code.map(ExitCode)
            ))),
        ]
    }

    fn any_stream() -> impl Strategy<Value = Vec<TerminalItem>> {
        prop::collection::vec(any_item(), 0..64)
    }

    proptest! {
        /// The property that matters most for this product: for a screen-reader terminal,
        /// silently dropping output is the cardinal defect. Every input character comes
        /// out, in order, whatever nonsense the markers around it did.
        #[test]
        fn text_is_never_lost(items in any_stream()) {
            let mut tracker = BoundaryTracker::new();
            let events = tracker.observe(items.clone());

            let given: String = items
                .iter()
                .filter_map(|item| match item {
                    TerminalItem::Text(text) => Some(text.as_str()),
                    TerminalItem::Marker(_) => None,
                })
                .collect();
            let emitted: String = events
                .iter()
                .filter_map(|event| match event {
                    BoundaryEvent::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();

            prop_assert_eq!(given, emitted);
        }

        #[test]
        fn blocks_are_balanced_and_never_nest(items in any_stream()) {
            let mut tracker = BoundaryTracker::new();
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
            let mut tracker = BoundaryTracker::new();
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
}
