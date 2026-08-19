//! Adapter (internal to the terminal-engine adapter): line extraction and line identity
//! over the emulator's grid.
//!
//! Text comes from the grid, never from the raw byte stream — DESIGN's Decided phasing
//! item, which is also what retires the question of which `Handler` methods the speech
//! path needs: the answer is none of them.
//!
//! Change is detected, not predicted. A row is only truly final once it has left the
//! active screen area; until then any row is reachable by cursor addressing, not just
//! the last one, which is how `docker pull` updates a stack of per-layer progress lines
//! and how `cargo` keeps a status line below its output — both on the primary screen,
//! no alternate screen involved. So this type keeps the text it last emitted for every
//! line still on screen and diffs against the grid after each advance: text that still
//! begins with what was emitted is an append, and anything else is a rewrite
//! (spec B3, decision 5).
//!
//! `Term::damage()` is deliberately not the mechanism. Its iterator filters out damage
//! that is not currently visible, so anything that scrolled past the viewport inside a
//! single batch — the normal case for a build log — would never be reported. Damage is
//! built for a renderer that only draws what is on screen; extraction has the opposite
//! requirement.

use std::collections::BTreeMap;
use std::mem::take;
use std::ops::Bound::{Excluded, Included};

use acter_core::{LineId, LineRevision, TerminalItem};
use alacritty_terminal::Term;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::Line;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{ClearMode, Handler};

/// What a user hears when output scrolled past the emulator's staging buffer before it
/// could be read. Silently dropping output is this product's cardinal defect; admitting
/// a gap is the honest degradation, exactly as B2's unstructured region is.
const SCROLLBACK_GAP: &str = "Some output was lost before it could be read: more lines \
                              arrived at once than the terminal can hold.";

/// A row the extractor is watching, and the text it last read there.
///
/// The identity is optional because a row outlives its line. When a block closes the
/// line is settled and its id retired, but the row keeps holding the same characters —
/// so the text is kept too, and only a row whose text actually *changes* after that
/// becomes a new line. Dropping the text along with the id would re-emit every frozen
/// row on the very next scan.
#[derive(Debug)]
struct Tracked {
    id: Option<LineId>,
    emitted: String,
}

/// Owns line identity for one session: the row-to-id map, the text last emitted for each
/// live line, the minting counter, and the row numbering the map is keyed on.
///
/// Rows are keyed by an **absolute** row number that survives scrolling, because a grid
/// line index does not: history grows by exactly what a scrolling line's index loses. It
/// survives scrollback eviction too, because this type does the evicting — see
/// [`Extractor::reclaim`].
#[derive(Debug)]
pub(super) struct Extractor {
    /// How many rows of history the emulator was configured to stage.
    staging_rows: usize,
    /// The next identity to mint. Monotonic for the life of the session, never reused.
    next_id: u64,
    /// Rows this type has dropped from the emulator's history, added back into every
    /// absolute row number so the numbering never shifts under the map.
    base: usize,
    /// The lowest absolute row that may still need emitting.
    scan_floor: usize,
    /// Live lines, keyed by the absolute row each one starts on. Ordered, so a scan
    /// emits rows top to bottom.
    lines: BTreeMap<usize, Tracked>,
}

impl Extractor {
    pub(super) fn new(staging_rows: usize) -> Self {
        Self {
            staging_rows,
            next_id: 0,
            base: 0,
            scan_floor: 0,
            lines: BTreeMap::new(),
        }
    }

    /// Emits everything the grid has gained or changed since the last call.
    pub(super) fn extract<T: EventListener>(
        &mut self,
        term: &mut Term<T>,
        out: &mut Vec<TerminalItem>,
    ) {
        let view = View::of(term, self.base);

        // The staging buffer saturated, so rows may have scrolled off the top of it
        // before they could be read. The map cannot be trusted afterwards — evicted rows
        // shift every row number under it — so the live lines are settled with the text
        // already emitted for them rather than with anything read back from a grid that
        // has moved, and the scan restarts from the oldest row still present.
        if view.saturated(self.staging_rows) {
            self.settle_from_record(out);
            let id = self.mint();
            out.push(item(id, SCROLLBACK_GAP.to_owned(), LineRevision::Settled));
            self.scan_floor = view.oldest;
        }

        // A cursor above the floor means the program went back to rows this epoch has
        // not emitted yet, which is how a full-screen application paints from the top
        // after a screen change.
        self.scan_floor = self.scan_floor.min(view.cursor).max(view.oldest);

        if let Some(end) = self.scan_end(&view) {
            let mut row = self.scan_floor;
            while row <= end {
                row = self.emit_line(&view, row, out) + 1;
            }
        }

        let (history, top) = (view.history, view.top);
        self.reclaim(term, history, top);
    }

    /// Settles every live line with its current text and retires its id, keeping the
    /// row's text so an unchanged row stays quiet.
    ///
    /// This is what a command block's end does. Ids stay session-global and outlive
    /// their block — a frontend has to be able to find a line whichever block produced
    /// it — but the right to *revise* one stops at the boundary, so a later rewrite of
    /// these rows starts new lines rather than mutating ones the user may already have
    /// reviewed. A review buffer that changes behind the reader is worse than a
    /// duplicate (spec B3, decision 7).
    pub(super) fn settle_block<T: EventListener>(
        &mut self,
        term: &Term<T>,
        out: &mut Vec<TerminalItem>,
    ) {
        let view = View::of(term, self.base);
        for (row, tracked) in &mut self.lines {
            let Some(id) = tracked.id.take() else {
                continue;
            };
            let (text, _) = view.read_line(*row);
            out.push(item(id, text.clone(), LineRevision::Settled));
            tracked.emitted = text;
        }
    }

    /// Settles every live line and forgets the rows entirely.
    ///
    /// This is what a screen change and a resize do, where the row numbering itself
    /// stops describing anything: the alternate screen is a separate grid with its own
    /// coordinate space, and a resize reflows.
    pub(super) fn settle_and_forget<T: EventListener>(
        &mut self,
        term: &Term<T>,
        out: &mut Vec<TerminalItem>,
    ) {
        let view = View::of(term, self.base);
        for (row, tracked) in take(&mut self.lines) {
            if let Some(id) = tracked.id {
                let (text, _) = view.read_line(row);
                out.push(item(id, text, LineRevision::Settled));
            }
        }
    }

    /// Starts a fresh epoch at the current cursor, after the grid has become something
    /// the old row numbering no longer describes.
    ///
    /// The cursor, not the top of the screen, because the rows above it are content this
    /// extractor has already emitted once: a resize reflows text that is still the same
    /// text, and the normal screen a program hands back still holds the session it was
    /// paused from. Anchoring higher would re-emit all of it as new lines.
    pub(super) fn reanchor<T: EventListener>(&mut self, term: &Term<T>) {
        self.scan_floor = View::of(term, self.base).cursor;
    }

    /// Starts a fresh epoch at the top of the screen area, for a grid whose content has
    /// never been emitted: the alternate screen, which mode 1049 clears on the way in.
    ///
    /// The cursor is the wrong anchor there, and the difference is not academic — a
    /// full-screen program's first write is `ESC[H ESC[2J`, painting from row zero
    /// upward of wherever the cursor happened to be. Anchoring at the cursor loses every
    /// row above it, which for `nano` is its title bar and for `vim` its first screenful
    /// of the file (found by the B3.5 pipeline test, which was the first caller to feed
    /// this engine a real repaint).
    pub(super) fn reanchor_to_top<T: EventListener>(&mut self, term: &Term<T>) {
        self.scan_floor = View::of(term, self.base).top;
    }

    /// Emits the line starting at `row` and returns the absolute row it ends on.
    fn emit_line(&mut self, view: &View<'_>, row: usize, out: &mut Vec<TerminalItem>) -> usize {
        let (text, last) = view.read_line(row);
        // A line is final once the whole of it, continuation rows included, has left the
        // screen area: nothing addressable can reach it any more.
        let settled = last < view.top;

        match self.lines.remove(&row) {
            // A live line: the ordinary diff.
            Some(Tracked {
                id: Some(id),
                emitted,
            }) => {
                if settled {
                    out.push(item(id, text, LineRevision::Settled));
                } else {
                    match text.strip_prefix(emitted.as_str()) {
                        Some("") => {}
                        Some(delta) => out.push(item(id, delta.to_owned(), LineRevision::Appended)),
                        None => out.push(item(id, text.clone(), LineRevision::Rewritten)),
                    }
                    self.lines.insert(
                        row,
                        Tracked {
                            id: Some(id),
                            emitted: text,
                        },
                    );
                }
            }
            // A frozen row. Unchanged, it has nothing to say; changed, it is a new line
            // carrying the row's whole text, because the line it used to be can no
            // longer be revised.
            Some(frozen) if frozen.emitted == text => {
                if !settled {
                    self.lines.insert(row, frozen);
                }
            }
            Some(_) | None => {
                let id = self.mint();
                if settled {
                    out.push(item(id, text, LineRevision::Settled));
                } else {
                    out.push(item(id, text.clone(), LineRevision::Appended));
                    self.lines.insert(
                        row,
                        Tracked {
                            id: Some(id),
                            emitted: text,
                        },
                    );
                }
            }
        }

        self.absorb(row, last, out);

        if settled {
            self.scan_floor = last + 1;
        }
        last
    }

    /// Retires the lines on rows a growing line has just swallowed as continuations.
    ///
    /// A row that held a line of its own before the row above it grew past the right
    /// margin no longer holds one: those characters are still on screen, but they belong
    /// to the line above now. The stream has no item for "this line is gone", so the
    /// swallowed line settles empty, which is the closest this model comes to saying so.
    /// It takes a cursor moved back up into existing content to reach at all.
    fn absorb(&mut self, row: usize, last: usize, out: &mut Vec<TerminalItem>) {
        if last == row {
            return;
        }
        let swallowed: Vec<usize> = self
            .lines
            .range((Excluded(row), Included(last)))
            .map(|(key, _)| *key)
            .collect();
        for key in swallowed {
            if let Some(Tracked { id: Some(id), .. }) = self.lines.remove(&key) {
                out.push(item(id, String::new(), LineRevision::Settled));
            }
        }
    }

    /// The last absolute row worth scanning, or `None` when there is nothing to emit.
    ///
    /// It walks up from the bottom of the screen area past the run of blank rows nobody
    /// has written to: minting a line for the fresh row a newline has just moved onto
    /// would announce a line that does not exist yet. It cannot simply stop at the
    /// cursor, because a program that moves the cursor back up leaves written rows below
    /// it. Blank rows above the last written one are kept, because vertical spacing is
    /// structure a user navigating by line depends on (decision 9).
    fn scan_end(&self, view: &View<'_>) -> Option<usize> {
        let mut end = view.bottom;
        while !self.lines.contains_key(&end) && view.row_is_blank(end) {
            if end == self.scan_floor {
                return None;
            }
            end -= 1;
        }
        Some(end)
    }

    /// Drops the history this scan has already emitted, adding it to the base so the
    /// absolute numbering carries on unbroken.
    ///
    /// Without this the numbering is only stable until the emulator's scrollback fills:
    /// after that `history_size` is pinned at its maximum and every further scroll shifts
    /// every row number by one, with no eviction count to read back. Reclaiming keeps the
    /// arithmetic exact, and it makes the saturation check above mean what it says —
    /// history starts each scan empty, so a full one is a real overflow rather than a
    /// long session. The emulator's history is a staging area for a single read; the
    /// user's scrollback is the buffer this stream feeds.
    fn reclaim<T: EventListener>(&mut self, term: &mut Term<T>, history: usize, top: usize) {
        // A line still straddling the top of the screen area — a wrapped line whose tail
        // is still addressable — has rows below it that must survive this round.
        if history == 0 || self.scan_floor < top {
            return;
        }
        term.clear_screen(ClearMode::Saved);
        self.base += history;
    }

    fn settle_from_record(&mut self, out: &mut Vec<TerminalItem>) {
        for (_, tracked) in take(&mut self.lines) {
            if let Some(id) = tracked.id {
                out.push(item(id, tracked.emitted, LineRevision::Settled));
            }
        }
    }

    fn mint(&mut self) -> LineId {
        let id = LineId(self.next_id);
        self.next_id += 1;
        id
    }
}

fn item(id: LineId, text: String, revision: LineRevision) -> TerminalItem {
    TerminalItem::Line { id, text, revision }
}

/// One scan's view of the grid: the geometry, plus reading rows out of it.
///
/// Absolute row numbers run from [`View::oldest`] (the top of history) down through
/// [`View::bottom`] (the last row of the screen area); [`View::top`] is the first row of
/// the screen area, so a row is in history exactly when it is above that.
struct View<'a> {
    grid: &'a Grid<Cell>,
    history: usize,
    oldest: usize,
    top: usize,
    bottom: usize,
    cursor: usize,
    alternate: bool,
}

impl<'a> View<'a> {
    fn of<T: EventListener>(term: &'a Term<T>, base: usize) -> Self {
        let grid = term.grid();
        let history = grid.history_size();
        let top = base + history;
        Self {
            history,
            oldest: base,
            top,
            bottom: top + grid.screen_lines() - 1,
            cursor: top.saturating_add_signed(grid.cursor.point.line.0 as isize),
            alternate: term.mode().contains(TermMode::ALT_SCREEN),
            grid,
        }
    }

    /// Whether history filled up during this batch, meaning rows may have been evicted
    /// before they were read. The alternate screen is exempt: it keeps no history at all
    /// by design, so a full one there says nothing about loss.
    fn saturated(&self, staging_rows: usize) -> bool {
        !self.alternate && self.history >= staging_rows
    }

    /// Reads the logical line starting at absolute row `row`, following the wrap flag,
    /// and returns it with the absolute row it ends on.
    ///
    /// A grid has a width and speech does not: a row whose last cell is flagged as
    /// wrapping continues into the next one, and joining them is what stops every
    /// sentence longer than the terminal is wide from reaching the screen reader broken
    /// at column eighty (decision 8).
    fn read_line(&self, row: usize) -> (String, usize) {
        let mut text = String::new();
        let mut last = row;
        loop {
            let wraps = self.read_row(last, &mut text);
            if !wraps || last >= self.bottom {
                break;
            }
            last += 1;
        }
        // A grid row is padded with spaces to its full width, so an untrimmed walk
        // speaks eighty spaces after every line (decision 9).
        let trimmed = text.trim_end().len();
        text.truncate(trimmed);
        (text, last)
    }

    /// Appends one row's characters and reports whether it wraps into the next.
    fn read_row(&self, row: usize, text: &mut String) -> bool {
        let line = &self.grid[self.line_of(row)];
        for cell in line {
            // The second half of a wide glyph, and the placeholder a wide glyph leaves
            // when it does not fit at the end of a row. Keeping either doubles every CJK
            // character.
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            text.push(cell.c);
            // Combining marks and accents hang off their base cell rather than occupying
            // one of their own.
            if let Some(zerowidth) = cell.zerowidth() {
                text.extend(zerowidth);
            }
        }
        line.last()
            .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE))
    }

    /// Whether a row holds nothing at all.
    fn row_is_blank(&self, row: usize) -> bool {
        let mut text = String::new();
        self.read_row(row, &mut text);
        text.trim_end().is_empty()
    }

    fn line_of(&self, row: usize) -> Line {
        Line((row as i64 - self.top as i64) as i32)
    }
}

/// Every logical line the grid holds, top of history through the last written row, under
/// the same reading rules extraction uses. The reference the emitted stream is checked
/// against in this crate's property tests.
#[cfg(test)]
pub(super) fn grid_lines<T: EventListener>(term: &Term<T>) -> Vec<String> {
    let view = View::of(term, 0);
    let mut lines = Vec::new();
    let mut row = view.oldest;
    while row <= view.bottom {
        let (text, last) = view.read_line(row);
        lines.push(text);
        row = last + 1;
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}
