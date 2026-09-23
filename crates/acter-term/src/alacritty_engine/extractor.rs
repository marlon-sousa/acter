//! Adapter (internal to the terminal-engine adapter): line extraction and line identity
//! over the emulator's grid.

use std::collections::BTreeMap;
use std::mem::take;
use std::ops::Bound::{Excluded, Included, Unbounded};

use acter_core::{LineId, LineRevision, TerminalItem};
use alacritty_terminal::Term;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::Line;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{ClearMode, Handler};

const SCROLLBACK_GAP: &str = "Some output was lost before it could be read: more lines \
                              arrived at once than the terminal can hold.";

/// `id` is `None` once the line has settled; the text stays so that only a row that changes
/// afterwards becomes a new line.
#[derive(Debug)]
struct Tracked {
    id: Option<LineId>,
    emitted: String,
}

/// Rows are keyed by an absolute number that survives scrolling and eviction, because
/// [`Extractor::reclaim`] does the evicting and adds what it evicts to `base`.
#[derive(Debug)]
pub(super) struct Extractor {
    staging_rows: usize,
    next_id: u64,
    base: usize,
    /// The lowest absolute row that may still need emitting.
    scan_floor: usize,
    /// Keyed by the absolute row each line starts on.
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

    pub(super) fn extract<T: EventListener>(
        &mut self,
        term: &mut Term<T>,
        out: &mut Vec<TerminalItem>,
    ) {
        let view = View::of(term, self.base);

        // Saturated history may have evicted unread rows and shifted every row number, so
        // live lines settle with the text already emitted rather than anything read back.
        if view.saturated(self.staging_rows) {
            self.settle_from_record(out);
            let id = self.mint();
            out.push(item(id, SCROLLBACK_GAP.to_owned(), LineRevision::Settled));
            self.scan_floor = view.oldest;
        }

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

    pub(super) fn reanchor<T: EventListener>(&mut self, term: &Term<T>) {
        self.scan_floor = View::of(term, self.base).cursor;
    }

    pub(super) fn reanchor_to_top<T: EventListener>(&mut self, term: &Term<T>) {
        self.scan_floor = View::of(term, self.base).top;
    }

    /// Emits the line starting at `row` and returns the absolute row it ends on.
    fn emit_line(&mut self, view: &View<'_>, row: usize, out: &mut Vec<TerminalItem>) -> usize {
        let (text, last) = view.read_line(row);
        // A line is final once all of it, continuation rows included, has left the screen area.
        let settled = last < view.top;

        match self.lines.remove(&row) {
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
            Some(frozen) if frozen.emitted == text => {
                if !settled {
                    self.lines.insert(row, frozen);
                }
            }
            Some(_) | None => {
                self.absorb(row, last, out);
                match self.take_place_below(last, out) {
                    Some((id, _)) if settled => out.push(item(id, text, LineRevision::Settled)),
                    Some((id, shown)) => {
                        match text.strip_prefix(shown.as_str()) {
                            Some("") => {}
                            Some(delta) => {
                                out.push(item(id, delta.to_owned(), LineRevision::Appended));
                            }
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
                    None => {
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
            }
        }

        self.absorb(row, last, out);

        if settled {
            self.scan_floor = last + 1;
        }
        last
    }

    /// A row swallowed as a continuation of the line above settles empty, because the stream
    /// has no item for a line that is gone.
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

    /// A reader places each id where it first appeared, so a new line above live lines takes the
    /// first one's id and hands every id one line down. `None` when no live line is below.
    fn take_place_below(
        &mut self,
        last: usize,
        out: &mut Vec<TerminalItem>,
    ) -> Option<(LineId, String)> {
        let below: Vec<usize> = self
            .lines
            .range((Excluded(last), Unbounded))
            .filter(|(_, tracked)| tracked.id.is_some())
            .map(|(key, _)| *key)
            .collect();
        let (&first, rest) = below.split_first()?;
        let taken = self
            .lines
            .get(&first)
            .and_then(|tracked| tracked.id.map(|id| (id, tracked.emitted.clone())))?;

        let mut receiver = first;
        for &next in rest {
            let (id, emitted) = {
                let tracked = &self.lines[&next];
                (tracked.id, tracked.emitted.clone())
            };
            self.lines.insert(receiver, Tracked { id, emitted });
            receiver = next;
        }
        let fresh = self.mint();
        out.push(item(fresh, String::new(), LineRevision::Appended));
        self.lines.insert(
            receiver,
            Tracked {
                id: Some(fresh),
                emitted: String::new(),
            },
        );
        Some(taken)
    }

    /// The last row worth scanning, skipping the unwritten blank rows at the bottom of the
    /// screen area, or `None` when there is nothing to emit.
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

    /// Drops history already emitted from the emulator and adds it to `base`, so a full history
    /// means a real overflow within one read.
    fn reclaim<T: EventListener>(&mut self, term: &mut Term<T>, history: usize, top: usize) {
        // A wrapped line whose tail is still on screen needs its history rows for another round.
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

/// Absolute rows run from `oldest` (top of history) to `bottom`; a row is in history when it is
/// above `top`.
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

    fn saturated(&self, staging_rows: usize) -> bool {
        !self.alternate && self.history >= staging_rows
    }

    /// Follows the wrap flag and returns the logical line with the absolute row it ends on.
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
        let trimmed = text.trim_end().len();
        text.truncate(trimmed);
        (text, last)
    }

    /// Appends one row's characters and reports whether it wraps into the next.
    fn read_row(&self, row: usize, text: &mut String) -> bool {
        let line = &self.grid[self.line_of(row)];
        for cell in line {
            // Keeping wide-glyph spacer cells would double every CJK character.
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            text.push(cell.c);
            if let Some(zerowidth) = cell.zerowidth() {
                text.extend(zerowidth);
            }
        }
        line.last()
            .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE))
    }

    fn row_is_blank(&self, row: usize) -> bool {
        let mut text = String::new();
        self.read_row(row, &mut text);
        text.trim_end().is_empty()
    }

    fn line_of(&self, row: usize) -> Line {
        Line((row as i64 - self.top as i64) as i32)
    }
}

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
