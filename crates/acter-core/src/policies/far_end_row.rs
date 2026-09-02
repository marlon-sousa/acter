//! Policy: which row the far end just redrew is the answer to the key Acter sent, and
//! where the caret goes in it.
//!
//! **A policy over events that already exist, and not a differ** (DESIGN, "A row that
//! changed is an answer"). The engine has emitted identified lines with `Appended` and
//! `Rewritten` revisions since B3 and already suppresses a repaint that changed nothing —
//! a `gh` prompt redrawing four rows after an arrow produces exactly two items, for the two
//! rows whose *text* differed. So the comparison was built three entries ago for another
//! reason; what was missing was permission to speak the result.
//!
//! Three bounds keep it small and all three already exist: only after a key Acter sent,
//! only once the batch settles on the quiescence clock the pacing policy computes, and only
//! over the rows the engine says changed.
//!
//! **Row count routes nothing** (spec 28, decision 6). PSReadLine's first arrow at a
//! completion menu changed eleven rows — the command line rewritten and ten menu rows
//! blanked — and it is ordinary Tab completion. A rule that sent "most of the screen
//! changed" somewhere else would mis-route the commonest thing anybody does in PowerShell.
//! The alternate screen is the only boundary that means anything, and it is phase 2's.

use crate::LineId;

/// One row the far end changed after a key Acter sent: what stood on it, and what stands
/// now.
///
/// Both texts, because step 2 is about *gaining* content rather than about having some. The
/// row losing a `gh` selection went from `> marlon-sousa/acter` to `  marlon-sousa/acter`
/// and the row gaining it went from `  Skip pushing the branch` to
/// `> Skip pushing the branch`; only the second is what a listener arrowing a list wants,
/// and only the pair of texts can tell them apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowChange {
    pub line: LineId,
    pub before: String,
    pub after: String,
}

/// The row the far end draws its command line on, and the column that line starts at.
///
/// **The anchor is why the prompt is not read aloud on every press.** What comes back on
/// the wire when a recalled line changes is a cursor address and the few characters that
/// differ — `readline` repaints from the column the line starts at — so the row the engine
/// then reports is `marlon@splyt:/mnt/c/Users/marlo$ exit`, prompt included, and what a
/// listener wants is `exit` (measured 2026-08-31).
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
}

/// What to put in front of the listener.
///
/// Text and a caret rather than a sentence, because the element holding them is an ARIA
/// text box and the reader does the speaking (spec 28, decision 3): NVDA answers the row
/// when the row changed, the character at the caret when only the cursor moved, and "blank"
/// for a row a key emptied — its own word, in a vocabulary its users already have. That is
/// why this type carries no strings Acter invented and never will.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FarEndAnswer {
    /// The row changed: this is its text, with the caret at this character.
    Row { text: String, caret: usize },
    /// Nothing was redrawn and the cursor moved along the row it was already on.
    Caret { caret: usize },
    /// Nothing the listener has any business hearing about.
    Nothing,
}

/// Which row is the answer, in the two steps decision 6 measured, and the two ways of
/// having no answer at all.
///
/// 1. **If the anchored row changed, that is the answer**, from the anchor column onward.
///    `readline`'s history recall and Tab completion, PSReadLine's completion menu — where
///    the anchored row wins over ten blanked ones — and every far end that has a command
///    line. Both kinds of change count: the first up arrow at a fresh prompt *appends* the
///    recalled line rather than rewriting the row, because there was nothing there to
///    overwrite, so a rule keyed on rewrites alone would be silent on the commonest press
///    of the commonest key (measured 2026-08-31).
/// 2. **Otherwise, among the rows that changed, the one that gained non-whitespace
///    content.** `gh`'s selection prompt, where the cursor is hidden and parked below the
///    list and the anchored row never changes at all. Content rather than a marker
///    character: naming `>` would hard-code one program's choice, and PSReadLine's menu —
///    the second prompt-driven sample — draws its selection in colour alone, with no marker
///    anywhere.
/// 3. **Otherwise, if the cursor is visible and moved along the row it was on**, only the
///    caret moves. Left, right, Home and End rewrite nothing and are invisible to steps 1
///    and 2 by construction.
/// 4. **Otherwise nothing happened.**
pub fn far_end_row(keystroke: &Keystroke<'_>) -> FarEndAnswer {
    if let Some(anchor) = keystroke.anchor
        && let Some(change) = keystroke
            .changed
            .iter()
            .find(|change| change.line == anchor.line)
    {
        let text = from_column(&change.after, anchor.column);
        let caret = caret_in(&text, anchor.column, keystroke.now);
        return FarEndAnswer::Row { text, caret };
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
            FarEndAnswer::Caret {
                caret: usize::from(now.column.saturating_sub(anchor)),
            }
        }
        _ => FarEndAnswer::Nothing,
    }
}

/// Whether this row gained non-whitespace content rather than losing it or trading it.
///
/// Counted rather than compared, so a program that marks its selection with `*`, with an
/// arrow, or by indenting the row is read the same way `gh`'s `>` is — and so a row that
/// merely had its text replaced with something of the same weight is not mistaken for one.
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
/// Clamped to the text, and placed at its end when the far end is not showing a cursor:
/// past the last character NVDA says "blank", which is the same word it says for a row a
/// key emptied and is exactly right for both.
fn caret_in(text: &str, anchor: u16, now: Option<Caret>) -> usize {
    let length = text.chars().count();
    match now {
        Some(caret) => usize::from(caret.column.saturating_sub(anchor)).min(length),
        None => length,
    }
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
        Keystroke {
            changed,
            anchor,
            was,
            now,
        }
    }

    fn anchored(line: u64, column: u16) -> Option<Anchor> {
        Some(Anchor {
            line: LineId(line),
            column,
        })
    }

    /// **`readline`'s history recall, from the transcript captured 2026-08-31.** The row is
    /// the prompt and the recalled line together; what a listener wants is the line, and the
    /// anchor is what separates them. Reading the row whole would say
    /// "marlon at splyt, slash mnt slash c..." before every single press.
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

    /// **The first up arrow at a fresh prompt appends rather than rewriting**, because
    /// there is nothing on the row to overwrite (measured 2026-08-31). A rule keyed on
    /// revisions would be silent here, on the commonest press of the commonest key — which
    /// is why this policy compares the row's content and never asks which revision brought
    /// it.
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

    /// Tab completion is the same shape, and it is the clearest argument for the anchor
    /// there is: Tab's whole contribution to the wire was the two bytes `o `, which is worth
    /// nothing spoken on its own.
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

    /// **PSReadLine's completion menu, captured 2026-09-02: the anchored row wins over ten
    /// blanked ones.** One arrow produced eleven line items — the command line rewritten and
    /// ten menu rows emptied — and what a listener wants is the one item the arrow selected,
    /// which PowerShell has already written onto the command line for them.
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

    /// **`gh`'s selection prompt, captured 2026-09-02: two rows change and the one that
    /// gained content wins.** The anchored row never changes — the user is answering a
    /// question rather than editing a line — and the cursor is hidden on the blank row below
    /// the list, so nothing but the content can choose between them.
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

    /// And it is the content rather than the marker, so a program that draws its highlight
    /// with a `*`, with an indent, or with a word is read the same way.
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

    /// **Left and right rewrite nothing at all, and the answer to them is a caret.** That is
    /// the whole reason the engine grew a cursor: these keys are invisible to every rule
    /// that watches rows change, and without a column there would be nothing to say about
    /// them.
    #[test]
    fn a_cursor_that_moved_along_its_row_moves_the_caret_and_nothing_else() {
        let answer = far_end_row(&keystroke(&[], anchored(7, 32), at(36, 3), at(35, 3)));
        assert_eq!(answer, FarEndAnswer::Caret { caret: 3 });
    }

    /// The caret is counted from the anchor, because that is where the text the listener
    /// has begins.
    #[test]
    fn the_caret_is_counted_from_the_anchor_column() {
        let answer = far_end_row(&keystroke(&[], anchored(7, 32), at(32, 3), at(33, 3)));
        assert_eq!(answer, FarEndAnswer::Caret { caret: 1 });
    }

    /// A cursor that changed rows is not a caret moving along a line — it is the far end
    /// having gone somewhere else — and nothing is said about it.
    #[test]
    fn a_cursor_that_changed_rows_is_not_a_caret_move() {
        let answer = far_end_row(&keystroke(&[], anchored(7, 0), at(4, 3), at(4, 4)));
        assert_eq!(answer, FarEndAnswer::Nothing);
    }

    /// A cursor the far end is not showing places nothing, which is `gh` for the whole of a
    /// selection.
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

    /// Nothing changed and nothing moved: the far end had no answer, and Acter invents none.
    #[test]
    fn nothing_changing_says_nothing() {
        let answer = far_end_row(&keystroke(&[], anchored(7, 0), at(4, 3), at(4, 3)));
        assert_eq!(answer, FarEndAnswer::Nothing);
    }

    /// **A row a key emptied is an empty row, and Acter says nothing about it.** `Ctrl+U`
    /// clears the line in `readline` and in PSReadLine and inserts a literal `^U` in
    /// `cmd.exe`, so what happened is the far end's business; what the listener gets is the
    /// row as it stands, which their reader calls "blank" in its own words.
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

    /// A row that only *lost* content is not an answer: it is the option a listener just
    /// left, and reading it beside the one they arrived at doubles every press.
    #[test]
    fn a_row_that_only_lost_content_is_not_the_answer() {
        let changed = [change(11, "> marlon-sousa/acter", "  marlon-sousa/acter")];
        assert_eq!(
            far_end_row(&keystroke(&changed, None, None, None)),
            FarEndAnswer::Nothing
        );
    }

    /// The anchored row wins even when another row gained more, because step 1 is asked
    /// first: a far end with a command line has already put the answer on it.
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

    /// A caret past the end of the text it is placed in is clamped to it, so a far end whose
    /// cursor is beyond what the grid holds cannot put the caret nowhere.
    #[test]
    fn a_caret_past_the_text_lands_at_its_end() {
        let changed = [change(7, "$ ", "$ ls")];
        let answer = far_end_row(&keystroke(&changed, anchored(7, 2), at(2, 3), at(99, 3)));

        assert_eq!(
            answer,
            FarEndAnswer::Row {
                text: "ls".to_owned(),
                caret: 2,
            }
        );
    }

    /// Pure: the same batch always answers the same thing.
    #[test]
    fn the_same_batch_always_answers_the_same_thing() {
        let changed = [change(7, "$ ", "$ ls")];
        let batch = keystroke(&changed, anchored(7, 2), at(2, 3), at(4, 3));
        assert_eq!(far_end_row(&batch), far_end_row(&batch));
    }
}
