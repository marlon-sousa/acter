//! Policy: which row the far end just redrew is the answer to the key Acter sent, and
//! where the caret goes in it.
//!
//! The engine already suppresses a repaint that changed nothing: a `gh` prompt redrawing
//! four rows after an arrow produces exactly two items, for the two rows whose text
//! differed. This policy only decides which of those already-filtered rows to speak.
//!
//! Runs only after a key Acter sent, once the batch settles on the pacing policy's
//! quiescence clock, over the rows the engine says changed.
//!
//! Row count routes nothing: PSReadLine's completion menu changes eleven rows for
//! ordinary Tab completion.

use crate::LineId;

/// One row the far end changed after a key Acter sent: what stood on it, and what stands
/// now.
///
/// Both texts, because gaining content is what step 2 looks for, not merely differing:
/// only the pair can tell a row that gained a selection marker from one that lost it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowChange {
    pub line: LineId,
    pub before: String,
    pub after: String,
}

/// The row the far end draws its command line on, and the column that line starts at.
///
/// Why the prompt is not read aloud on every press: `readline` repaints from the column
/// the line starts at, so the engine reports the row as `marlon@splyt:...$ exit`, prompt
/// included, when what a listener wants is `exit` (measured 2026-08-31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    /// The row the far end's cursor sat on when it finished drawing its prompt.
    pub line: LineId,
    /// The column it sat at, which is where the command line begins.
    pub column: u16,
}

/// Where the far end's cursor was when the key went out, and where it is now.
///
/// `None` for a far end that is not showing one: `gh` hides the cursor for the whole of a
/// selection and parks it on the blank row below the list, so a caret placed from it would
/// put a listener somewhere the far end never went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caret {
    pub column: u16,
    pub row: u16,
}

/// Everything one settled batch knows: what changed, where the command line starts, and
/// what the cursor did.
#[derive(Debug, Clone)]
pub struct Keystroke<'a> {
    /// The rows whose text differs from what stood on them when the key went out.
    pub changed: &'a [RowChange],
    /// The anchored row, or `None` when the far end has not drawn a command line Acter
    /// could anchor to — which is every widget that took the screen without one.
    pub anchor: Option<Anchor>,
    /// The visible cursor as the key went out, and as the batch settled. Either side is
    /// `None` when the far end was hiding it.
    pub was: Option<Caret>,
    pub now: Option<Caret>,
    /// What the field is holding right now — the text this policy handed over last time.
    ///
    /// The only record of trailing spaces: the extractor trims every row, so a row drawn
    /// as `echo hi ` reaches here as `echo hi`. `held` with its own padding removed is the
    /// row as the extractor read it.
    pub held: &'a str,
}

/// What to put in front of the listener.
///
/// Text and a caret, not a sentence: the element holding them is an ARIA text box, so NVDA
/// does the speaking — the row when it changed, the character at the caret when only the
/// cursor moved, "blank" for a row a key emptied. This type carries no strings Acter
/// invented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FarEndAnswer {
    /// The row changed: this is its text, with the caret at this character.
    Row { text: String, caret: usize },
    /// Nothing was redrawn and the cursor moved along the row it was already on.
    Caret { caret: usize },
    /// Nothing the listener has any business hearing about.
    Nothing,
}

/// Which row is the answer, in four steps.
///
/// 1. If the anchored row changed, that is the answer, from the anchor column onward.
///    Both kinds of change count: the first up arrow at a fresh prompt appends the
///    recalled line rather than rewriting the row, so a rule keyed on rewrites alone
///    would be silent on the commonest press of the commonest key (measured 2026-08-31).
/// 2. Otherwise, among the rows that changed, the one that gained non-whitespace content.
///    Content rather than a marker character: naming `>` would hard-code one program's
///    choice, and PSReadLine's completion menu draws its selection in colour alone, with
///    no marker at all.
/// 3. Otherwise, if the cursor is visible and moved along the row it was on, only the
///    caret moves — unless it moved across whitespace the extractor trimmed away, which
///    is this step's second half.
/// 4. Otherwise nothing happened.
///
/// Every row this rule hands over is padded out to the caret. A caret beyond the end of
/// the text is evidence in itself: the far end's cursor is a screen column, the grid under
/// it holds spaces, and the extractor trims them off the row. Padding puts the space back
/// where the cursor says it is, so deleting it is a change a reader can announce, and a
/// caret never sits past the text it is given.
pub fn far_end_row(keystroke: &Keystroke<'_>) -> FarEndAnswer {
    if let Some(anchor) = keystroke.anchor
        && let Some(change) = keystroke
            .changed
            .iter()
            .find(|change| change.line == anchor.line)
    {
        let text = from_column(&change.after, anchor.column);
        let caret = caret_in(&text, anchor.column, keystroke.now);
        return FarEndAnswer::Row {
            text: padded(text, caret),
            caret,
        };
    }

    if let Some(change) = keystroke.changed.iter().find(gained_content) {
        // The caret sits at the start rather than being placed from a cursor that is
        // somewhere else entirely: this row is an answer the far end drew, not a line the
        // user is editing.
        return FarEndAnswer::Row {
            text: change.after.clone(),
            caret: 0,
        };
    }

    match (keystroke.was, keystroke.now) {
        (Some(was), Some(now)) if was.row == now.row && was.column != now.column => {
            let anchor = keystroke.anchor.map_or(0, |anchor| anchor.column);
            let caret = usize::from(now.column.saturating_sub(anchor));
            // Nothing was redrawn, so the row is what the listener already has with this
            // policy's own padding removed, and where the cursor rests is the only
            // evidence of how much is still there. A cursor at or past the row's end
            // re-measures the padding — telling a space typed at the end from one deleted
            // from it; a cursor that landed inside the row says nothing about the
            // whitespace after it, so the line is left exactly as it was.
            let row = keystroke.held.trim_end();
            let text = if caret >= row.chars().count() {
                padded(row.to_owned(), caret)
            } else {
                keystroke.held.to_owned()
            };
            if text == keystroke.held {
                FarEndAnswer::Caret { caret }
            } else {
                FarEndAnswer::Row { text, caret }
            }
        }
        _ => FarEndAnswer::Nothing,
    }
}

/// Whether this row gained non-whitespace content rather than losing it or trading it.
///
/// Counted rather than compared: a program marking its selection with `*`, an arrow, or
/// indentation reads the same way `gh`'s `>` does, and a row merely rewritten with
/// same-weight text is not mistaken for one that gained content.
fn gained_content(change: &&RowChange) -> bool {
    weight(&change.after) > weight(&change.before)
}

fn weight(text: &str) -> usize {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .count()
}

/// The row from the anchor column onward.
///
/// Counted in characters rather than bytes, because a column is a screen position and the
/// text is what the extractor read out of the grid.
fn from_column(row: &str, column: u16) -> String {
    row.chars().skip(usize::from(column)).collect()
}

/// Where the caret goes in the text a listener is about to be handed.
///
/// Placed at the end of the text when the far end is not showing a cursor: past the last
/// character NVDA says "blank", the same word it says for a row a key emptied.
///
/// Not clamped, because the text is what gets adjusted: a cursor beyond the row's last
/// character is where a listener's own caret belongs after they type a space, and
/// clamping it back onto the text was what made that space vanish.
fn caret_in(text: &str, anchor: u16, now: Option<Caret>) -> usize {
    match now {
        Some(caret) => usize::from(caret.column.saturating_sub(anchor)),
        None => text.chars().count(),
    }
}

/// The row padded with spaces out to the caret, and left alone when the caret is already
/// in it.
///
/// The spaces are not invented: the cursor sits on a grid cell, and every cell between the
/// row's last character and that one holds a space that the extractor trims off before the
/// row reaches here. Trimming stays right for reading a transcript; the field needs the
/// row as wide as the far end's own cursor.
fn padded(text: String, caret: usize) -> String {
    let length = text.chars().count();
    let mut text = text;
    if caret > length {
        text.push_str(&" ".repeat(caret - length));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(line: u64, before: &str, after: &str) -> RowChange {
        RowChange {
            line: LineId(line),
            before: before.to_owned(),
            after: after.to_owned(),
        }
    }

    fn at(column: u16, row: u16) -> Option<Caret> {
        Some(Caret { column, row })
    }

    fn keystroke<'a>(
        changed: &'a [RowChange],
        anchor: Option<Anchor>,
        was: Option<Caret>,
        now: Option<Caret>,
    ) -> Keystroke<'a> {
        holding("", changed, anchor, was, now)
    }

    /// The same batch, with the field already holding a line — which is what the caret
    /// steps read the row's trailing whitespace out of.
    fn holding<'a>(
        held: &'a str,
        changed: &'a [RowChange],
        anchor: Option<Anchor>,
        was: Option<Caret>,
        now: Option<Caret>,
    ) -> Keystroke<'a> {
        Keystroke {
            changed,
            anchor,
            was,
            now,
            held,
        }
    }

    fn anchored(line: u64, column: u16) -> Option<Anchor> {
        Some(Anchor {
            line: LineId(line),
            column,
        })
    }

    /// `readline`'s history recall, captured 2026-08-31. Reading the whole row would speak
    /// "marlon at splyt, slash mnt slash c..." before every press.
    #[test]
    fn history_recall_speaks_the_line_and_not_the_prompt() {
        let prompt = "marlon@splyt:/mnt/c/Users/marlo$ ";
        let changed = [change(
            7,
            &format!("{prompt}echo acter-history-one"),
            &format!("{prompt}exit"),
        )];
        let answer = far_end_row(&keystroke(
            &changed,
            anchored(7, prompt.chars().count() as u16),
            at(55, 3),
            at(37, 3),
        ));

        assert_eq!(
            answer,
            FarEndAnswer::Row {
                text: "exit".to_owned(),
                caret: 4,
            }
        );
    }

    #[test]
    fn the_first_recall_is_an_append_and_is_still_the_answer() {
        let prompt = "marlon@splyt:/mnt/c/Users/marlo$ ";
        let changed = [change(
            7,
            prompt,
            &format!("{prompt}echo acter-history-one"),
        )];
        let anchor = prompt.chars().count() as u16;
        let answer = far_end_row(&keystroke(
            &changed,
            anchored(7, anchor),
            at(anchor, 3),
            at(anchor + 22, 3),
        ));

        assert_eq!(
            answer,
            FarEndAnswer::Row {
                text: "echo acter-history-one".to_owned(),
                caret: 22,
            }
        );
    }

    /// Tab's whole contribution to the wire is two bytes, `o `, worth nothing spoken alone.
    #[test]
    fn tab_completion_speaks_the_completed_line_rather_than_what_tab_added() {
        let prompt = "marlon@splyt:~$ ";
        let changed = [change(
            4,
            &format!("{prompt}ech"),
            &format!("{prompt}echo "),
        )];
        let answer = far_end_row(&keystroke(&changed, anchored(4, 16), at(19, 2), at(21, 2)));

        assert_eq!(
            answer,
            FarEndAnswer::Row {
                text: "echo ".to_owned(),
                caret: 5,
            }
        );
    }

    #[test]
    fn a_menu_repaint_answers_with_the_command_line_and_not_the_menu() {
        let prompt = "PS C:\\Users\\marlo> ";
        let mut changed = vec![change(
            2,
            &format!("{prompt}Get-CIPolicyInfo"),
            &format!("{prompt}Get-CertificateAutoEnrollmentPolicy"),
        )];
        for row in 3..13 {
            changed.push(change(row, "Get-Command                    ", ""));
        }
        let answer = far_end_row(&keystroke(&changed, anchored(2, 19), at(35, 0), at(54, 0)));

        assert_eq!(
            answer,
            FarEndAnswer::Row {
                text: "Get-CertificateAutoEnrollmentPolicy".to_owned(),
                caret: 35,
            }
        );
    }

    #[test]
    fn a_selection_prompt_answers_with_the_row_that_gained_content() {
        let changed = [
            change(11, "> marlon-sousa/acter", "  marlon-sousa/acter"),
            change(12, "  Skip pushing the branch", "> Skip pushing the branch"),
        ];
        let answer = far_end_row(&keystroke(&changed, anchored(9, 0), None, None));

        assert_eq!(
            answer,
            FarEndAnswer::Row {
                text: "> Skip pushing the branch".to_owned(),
                caret: 0,
            }
        );
    }

    #[test]
    fn the_rule_is_content_and_never_a_marker_character() {
        let changed = [
            change(4, "* Ubuntu", "  Ubuntu"),
            change(5, "  Debian", "* Debian"),
        ];
        assert_eq!(
            far_end_row(&keystroke(&changed, None, None, None)),
            FarEndAnswer::Row {
                text: "* Debian".to_owned(),
                caret: 0,
            }
        );
    }

    #[test]
    fn a_cursor_that_moved_along_its_row_moves_the_caret_and_nothing_else() {
        let answer = far_end_row(&holding("exit", &[], anchored(7, 32), at(36, 3), at(35, 3)));
        assert_eq!(answer, FarEndAnswer::Caret { caret: 3 });
    }

    #[test]
    fn the_caret_is_counted_from_the_anchor_column() {
        let answer = far_end_row(&holding("exit", &[], anchored(7, 32), at(32, 3), at(33, 3)));
        assert_eq!(answer, FarEndAnswer::Caret { caret: 1 });
    }

    #[test]
    fn a_cursor_that_changed_rows_is_not_a_caret_move() {
        let answer = far_end_row(&keystroke(&[], anchored(7, 0), at(4, 3), at(4, 4)));
        assert_eq!(answer, FarEndAnswer::Nothing);
    }

    #[test]
    fn a_hidden_cursor_moves_no_caret() {
        assert_eq!(
            far_end_row(&keystroke(&[], anchored(7, 0), at(4, 3), None)),
            FarEndAnswer::Nothing
        );
        assert_eq!(
            far_end_row(&keystroke(&[], anchored(7, 0), None, at(4, 3))),
            FarEndAnswer::Nothing
        );
    }

    #[test]
    fn nothing_changing_says_nothing() {
        let answer = far_end_row(&keystroke(&[], anchored(7, 0), at(4, 3), at(4, 3)));
        assert_eq!(answer, FarEndAnswer::Nothing);
    }

    #[test]
    fn a_row_a_key_emptied_is_reported_empty_and_not_described() {
        let prompt = "$ ";
        let changed = [change(7, &format!("{prompt}some command"), prompt)];
        let answer = far_end_row(&keystroke(&changed, anchored(7, 2), at(14, 3), at(2, 3)));

        assert_eq!(
            answer,
            FarEndAnswer::Row {
                text: String::new(),
                caret: 0,
            }
        );
    }

    #[test]
    fn a_row_that_only_lost_content_is_not_the_answer() {
        let changed = [change(11, "> marlon-sousa/acter", "  marlon-sousa/acter")];
        assert_eq!(
            far_end_row(&keystroke(&changed, None, None, None)),
            FarEndAnswer::Nothing
        );
    }

    #[test]
    fn the_anchored_row_is_asked_before_the_content_rule() {
        let changed = [
            change(9, "$ ", "$ exit"),
            change(11, "", "an entire row of other text"),
        ];
        let answer = far_end_row(&keystroke(&changed, anchored(9, 2), at(2, 1), at(6, 1)));

        assert_eq!(
            answer,
            FarEndAnswer::Row {
                text: "exit".to_owned(),
                caret: 4,
            }
        );
    }

    /// Tab completing a unique match in `readline` appends a trailing space that the grid
    /// holds and the extractor trims off; the cursor is the only evidence it was ever
    /// there.
    #[test]
    fn a_caret_past_the_text_pads_the_row_out_to_it() {
        let prompt = "marlon@splyt:~$ ";
        let changed = [change(4, &format!("{prompt}ech"), &format!("{prompt}echo"))];
        let answer = far_end_row(&holding(
            "ech",
            &changed,
            anchored(4, 16),
            at(19, 2),
            at(21, 2),
        ));

        assert_eq!(
            answer,
            FarEndAnswer::Row {
                text: "echo ".to_owned(),
                caret: 5,
            }
        );
    }

    /// Measured at a real `bash` 2026-09-02: typing a space after `echo hi` produced no
    /// line item, only the cursor moving one column right.
    #[test]
    fn a_space_typed_at_the_end_reaches_the_field_as_a_space() {
        let answer = far_end_row(&holding(
            "echo hi",
            &[],
            anchored(7, 16),
            at(23, 3),
            at(24, 3),
        ));

        assert_eq!(
            answer,
            FarEndAnswer::Row {
                text: "echo hi ".to_owned(),
                caret: 8,
            }
        );
    }

    /// Backspace over a trailing space produces no line item either, so the row alone
    /// cannot distinguish a space typed from one deleted.
    #[test]
    fn deleting_a_trailing_space_shortens_the_line_the_listener_holds() {
        let answer = far_end_row(&holding(
            "echo hi ",
            &[],
            anchored(7, 16),
            at(24, 3),
            at(23, 3),
        ));

        assert_eq!(
            answer,
            FarEndAnswer::Row {
                text: "echo hi".to_owned(),
                caret: 7,
            }
        );
    }

    #[test]
    fn a_caret_moving_inside_a_padded_line_still_rewrites_nothing() {
        let answer = far_end_row(&holding(
            "echo hi ",
            &[],
            anchored(7, 16),
            at(24, 3),
            at(20, 3),
        ));

        assert_eq!(answer, FarEndAnswer::Caret { caret: 4 });
    }

    #[test]
    fn the_same_batch_always_answers_the_same_thing() {
        let changed = [change(7, "$ ", "$ ls")];
        let batch = keystroke(&changed, anchored(7, 2), at(2, 3), at(4, 3));
        assert_eq!(far_end_row(&batch), far_end_row(&batch));
    }
}
