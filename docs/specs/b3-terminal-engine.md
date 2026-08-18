# Spec: PR B3 — the terminal engine adapter

Agreed in conversation 2026-08-17, and amended during implementation where the amendments
are marked inline — decisions 5, 6 and 7, the port's `resize`, and the Tests section. Lane
2, entry B3. Delivers `acter-term`: bytes in;
identified lines of extracted text, recognized OSC 133 markers, and alt-screen transitions
out as one ordered stream, behind a new `TerminalEngine` driven port. This is B2's first
caller, and it amends B2 — see decisions 6 and 7.

The unhandled-OSC fork this depends on already landed (PR #17, ahead of this spec, because
it was a dependency change rather than a component).

## Why now / relation to the roadmap

- B2's `BoundaryTracker` consumes a `TerminalItem` stream and nothing produces one. It is
  a policy with no caller, which is the situation B1.5 recorded as a mistake to avoid
  repeating.
- B2 moved OSC 133 **recognition** here by decision, on the grounds that this crate
  already runs a real escape-sequence parser and a second parser could disagree with the
  first about what is a real sequence.
- DESIGN's phasing decision 1 requires a real emulation core in phase 1, with the
  non-interactive text view derived from the grid. Nothing implements that yet.
- Alt-screen detection is a phase-1 guardrail in the same decision: without it, a user who
  types `nano` gets a session that looks hung.

## A Decided item this spec follows, and what it retires

Conversation on 2026-08-17 sketched a "small handler implementing only what the speech
path needs" — an enumerated subset of `ansi::Handler` (`input`, `carriage_return`,
`clear_line`, `erase_chars`, and so on) accumulating text directly, with the subset pinned
by a differential test against `Term`'s grid.

**That sketch is dropped, because DESIGN already decided against it.** Phasing decision 1,
marked Decided, says all output runs through a real emulation core in phase 1 and that
"the non-interactive text view is derived from the grid/scrollback, never from
regex-stripping ANSI out of the raw stream. In phase 1 the grid is a text extractor; in
phase 2 the same grid becomes the interactive screen." A hand-written accumulator is not
regex-stripping, but it is a second, weaker emulator standing where the grid was supposed
to stand — and it would diverge from the grid exactly where phase 2 later needs them to
agree. Raising it rather than quietly building it is the point: this spec does not
relitigate the decision, it follows it.

The consequence is worth stating plainly, because it removes work rather than adding it:
**the "which `Handler` methods does the speech path need" question is retired, not
answered.** Text comes from the grid, so the answer is "none of them". What the roadmap
entry called the fallback — grid-based extraction at marker boundaries — is the design.

## Design decisions this spec makes

1. **Two `Processor`s over the same bytes, and the second one implements three methods.**
   One `Processor` drives a real `Term` with zero forwarding. A second drives a small
   sniffer whose entire job is stream position: `unhandled_osc` (the fork's hook, for OSC
   133), plus `set_private_mode` and `unset_private_mode` (for `1049`, which is how
   alt-screen actually arrives — there is no dedicated `Handler` method for it). The
   remaining sixty-nine methods stay default no-ops, and here that is *correct* rather
   than merely convenient: the sniffer models nothing, so a method it does not implement
   is a method it has no business implementing.

   Rejected: one `Processor` with a wrapper owning a `Term` and forwarding all 72
   `Handler` methods. It works today and fails quietly later. Every `Handler` method has a
   default body, so when a future vte release *adds* one, the wrapper keeps compiling,
   silently picks up the new default no-op, and stops forwarding that capability to
   `Term`. No compile error, no failing test, just emulation that is subtly wrong until
   someone notices a rendering bug months later. vte has done exactly this before, in
   consecutive releases: 0.13.0 added `set_private_mode`, `unset_private_mode`,
   `report_mode` and `report_private_mode`, and 0.13.1 added SCP control support. This is
   a pattern, not a hypothetical. Keeping it safe would mean auditing 72 forwards against
   the diff on every vte bump, forever. Nothing forwards here, so nothing can be
   forgotten.

   The cost accepted knowingly is one extra DFA pass over each chunk, which is cheap next
   to the grid mutation the same bytes cause in `Term`.

   One residual to record rather than discover: a `Processor` holds synchronized-update
   (DCS 2026) timeout state, so two instances could in principle flush buffered output at
   different moments. Both are advanced back to back over the same slice, so the window is
   microseconds against a timeout measured in hundreds of milliseconds, and it
   self-corrects on the next chunk. Named here so a future reader knows it was considered.

2. **Stream position comes from the sniffer; state comes from `Term`.** `Term::mode()`
   containing `TermMode::ALT_SCREEN` remains the authority on which screen is current, and
   the engine reports it that way. What `Term` cannot supply is *where* in the stream the
   switch happened, and that matters: a single read from a PTY routinely carries
   `ESC[?1049h` followed immediately by the app's first full repaint, because that is what
   `vim` and `nano` write on startup. Polling the mode after the batch cannot tell which
   text preceded the switch, so the repaint would be attributed to the finished command and
   spoken as its output. Speaking a screenful of `vim` chrome at a user who typed `vim` is
   a real defect, not a cosmetic one.

3. **Alt-screen transitions travel in the item stream, so `TerminalItem` gains a variant.**
   `TerminalItem::ScreenChanged(Screen)` joins `Marker` and the `Line` item decision 6
   defines. The item stream is
   already the ordered thing, `Screen` is already a core entity, and the actor already
   models `AltScreenEntered` / `AltScreenLeft` as inputs sitting alongside `Output` — so
   one ordered stream from engine to actor is the shape both ends already have.
   `terminal_item.rs` anticipated this in writing ("these types will belong to the
   `TerminalEngine` port's contract once that port exists in B3"), but the amendment is
   still an amendment and rides in this PR, per the process rule.

   `BoundaryTracker` gains one arm for the new variant, passing it through as
   `BoundaryEvent::ScreenChanged(Screen)`. That is consistent with B2's own decision 2 —
   the tracker "drops nothing, rewrites nothing" — and it keeps ordering in one place.
   Rejected: a separate query or side channel for screen state, which loses the ordering
   decision 2 exists to preserve; and making the caller split each batch at transitions and
   call `observe` on the runs between, which pushes ordering reconstruction into every
   caller to save two lines in the tracker.

4. **Device replies must leave the engine, or programs hang.** `Term` does not answer
   device queries itself; it asks its `EventListener` to, emitting `Event::PtyWrite` at
   eight sites in `term/mod.rs` for the likes of DA and DSR and cursor-position reports,
   plus `Event::ColorRequest` and `Event::TextAreaSizeRequest`, each carrying a formatter
   closure that turns the answer into the right escape sequence. Pass `VoidListener` and
   every one of those is silently dropped — so a program that queries the terminal and
   waits for an answer waits forever. For this product that surfaces as a session that has
   simply gone quiet, with nothing to announce and no way for the user to tell why.

   So the engine owns an `EventListener` that captures them, answers the two formatter
   cases from the dimensions it already knows, and exposes the resulting bytes for the
   caller to write back to the transport. `Event::Bell` and `Event::Title` are captured and
   dropped with a comment naming who wants them later (a bell is DESIGN's beep; the title
   is not phase 1). Clipboard events cannot arise: `Config::osc52` is set to the
   disabled variant, which is also the right default for a terminal that has no clipboard
   story yet.

   Actually writing those bytes to the PTY is the caller's, so it lands with `Transport`
   in B3.5. The port exposes them; nothing in this PR consumes them, which is stated in
   Out of scope rather than left as a surprise.

5. **Extraction is line-oriented over absolute rows, and never uses `damage()`.** The
   engine keeps a cursor of its own: the absolute row and column it has already emitted
   text up to. An absolute row is `grid.history_size() + line.0`, which is stable while a
   line scrolls, because history grows by exactly what the line index loses.

   **A row is only truly final once it has scrolled out of the active screen area.** Until
   then any row is reachable by cursor addressing, not just the last one: `docker pull`
   updates a stack of per-layer progress lines in place with cursor-up, and `cargo` keeps a
   status line below its output — both on the primary screen, no alt-screen involved. So
   "rows above the cursor are done" is simply false, and the engine detects change instead
   of predicting it. It keeps the text it last emitted for each row still on screen — a
   screenful, tens of rows — and diffs against the grid after each advance. If a row's
   current text still begins with what was already emitted, the difference is an **append**.
   If it does not, the row was **rewritten**. That is a pure grid diff needing no `Handler`
   involvement, so it stays consistent with retiring the subset question, and it costs one
   screenful of character comparisons per read.

   `Term::damage()` is deliberately not the mechanism. `TermDamageIterator` filters out
   damage that is not currently visible, so anything that scrolled past the viewport inside
   a single batch — which is the normal case for a build log — would never be reported.
   Damage is built for a renderer that only draws what is on screen; extraction has the
   opposite requirement.

   The scrollback bound is the one place text could genuinely be lost: if a single batch
   scrolls more rows than `Config::scrolling_history`, evicted rows are gone before anyone
   read them. Two mitigations, both required. Size the history comfortably above the
   worst-case rows a single read can produce, and **detect** the overflow instead of
   trusting the arithmetic: if rows may have been evicted unread, the engine emits a `Line`
   saying so in a speakable sentence rather than silently skipping. Silently dropping
   output is this product's cardinal defect; admitting a gap is the honest degradation,
   exactly as B2's `Unstructured` region is.

   **Amended during implementation: the engine reclaims the emulator's history after each
   scan, and adds what it dropped to a base of its own.** The absolute row above is stable
   only while history is still growing. Once `scrolling_history` is reached, `history_size`
   is pinned at its maximum and every further scroll shifts every row number by one, with
   no eviction count anywhere in the API to read back — so in any session long enough to
   fill the scrollback, the row-to-id map would start misattributing, and would keep doing
   so. Reclaiming makes the arithmetic exact for the whole session, and it makes the
   overflow check mean what it says: history starts every scan empty, so finding it full is
   a real overflow rather than a long session. The emulator's history is a staging area for
   a single read; the *user's* scrollback is the buffer this stream feeds, and the consumer
   owns it. The consequence to record is that the grid keeps no scrollback of its own,
   which phase 2's renderer would have to provide for itself — consistent with decision 10,
   which already says phase 2 adds what it needs when it exists.

6. **Revision is represented, not duplicated: every line carries an identity.** Early
   emission is not optional — a row that stays on screen would otherwise never be spoken
   at all, and an unterminated row must be spoken, because DESIGN's quiescence auto-read
   exists precisely to say "Password:" half a second after it appears and that text never
   ends a line. So revision is inherent, and the only real question is how it is
   represented. An earlier draft of this spec answered "as duplicate text, held back until
   it settles". That answer is dropped: the stream carries a line identity, and a rewrite
   is a **revision of that line** rather than a second copy of it.

   A text item is therefore `Line { id, text, revision }`, with three revision kinds:

   - **`Appended`** — `text` is the delta added to the end of the line. The ordinary case:
     text streaming in.
   - **`Rewritten`** — the line changed below what was already emitted (decision 5's diff
     said so); `text` is the whole line.
   - **`Settled`** — the line can no longer change; `text` is its final content. Every line
     settles at most once, and nothing follows a line's settlement — so both the engine and
     every consumer have one clean point at which to drop per-line state.

   **Amended during implementation: a newline does not settle a line.** An earlier draft of
   this list said it did, which contradicts decision 5 and the multi-row test below: a
   newline-terminated row stays reachable by cursor addressing, and rewriting exactly such
   rows is what `docker pull` and `cargo`'s status stack do. Settling at the newline would
   turn every one of those rewrites into a fresh line, which is the buffer full of
   near-identical copies that DESIGN's Decided item exists to prevent. So a line settles
   only where change has become impossible: it scrolled out of the screen area, its command
   block closed, the screen changed, or the terminal was resized. Session teardown is not
   in the list because the port has no teardown method to hang it on; a consumer drops its
   per-line state when the session ends, as it must anyway.

   Two kinds would be information-sufficient: full text plus a final flag lets a consumer
   diff for itself. Three are carried anyway, for two reasons. The engine already did that
   diff in decision 5, so re-deriving it downstream would make **every** consumer keep a
   copy of every line. And because `Appended` carries only the delta, a session containing
   no rewrites produces exactly the append-only stream that exists today — so the ripple
   lands only on sessions that actually contain a rewrite, instead of on all of them.

   What each path does with it follows DESIGN's separate-paths decision exactly, which is
   the real argument for this shape. The **buffer** assigns or appends by id: always
   current, zero duplication, and the tension the withholding design had with "the buffer
   loads whenever content arrives" disappears rather than being argued around. **Speech**
   takes `Appended` as it does today, ignores `Rewritten` as buffer-only churn, and takes
   `Settled` as the line's final word. So a spinner never reaches speech mid-spin, and its
   result still does.

   What this deletes from this spec matters as much as what it adds: the holding of
   volatile rows, the stability-across-flushes rule, the hold cap, and `flush` on the port
   are all gone. Every one of them existed to keep unsettled text away from a buffer that
   had no way to revise it. With revision, nothing is withheld — the trailing row is
   emitted as it grows, on `advance` — and the one constant that had been picked by analogy
   rather than from evidence goes with them.

7. **Identity is opaque, and a block boundary freezes its lines.** A `LineId` is minted by
   the engine when a row is first emitted, monotonic, never reused.

   It is deliberately **not** a grid coordinate. The absolute row of decision 5 survives
   scrolling, but it breaks on resize (reflow moves rows), on scrollback eviction, and on
   alt-screen entry, which swaps in a separate grid with its own coordinate space. The
   engine keeps the row-to-id map internally, bounded by the screen, and the protocol never
   learns that a grid exists — the same discipline `CommandId` already follows.

   Ids are **session-global and outlive their block**: a frontend has to be able to find a
   line whichever block produced it, and a block-scoped id would leave it unable to.

   The update *capability*, though, stops at the boundary. When a marker closes a block the
   engine retires those rows' ids, so a later rewrite of the same screen rows emits new
   lines rather than mutating history the user may already have reviewed. A review buffer
   that changes behind the reader is worse than a duplicate. The same rule applies at a
   resize and at a screen change, where the old mapping is not meaningful anyway.

   **Amended during implementation, in two places.** First, a block end retires the id but
   *keeps* the row's text: dropping both would make the next scan see rows it has no record
   of and re-emit every one of them as a new line immediately, rather than only when
   something actually changes. A resize and a screen change do drop both, because there the
   row numbering itself stops describing anything — the alternate screen is a separate grid
   with its own coordinate space, and a resize reflows. Second, only a command end (`D`)
   freezes. A prompt start or a command start would freeze the row the prompt is drawn on,
   which is the same row the shell then echoes the command onto, so the echo would arrive
   as a second line repeating the prompt.

   One case the model cannot express, recorded rather than hidden: when a line grows past
   the right margin onto a row that already held a line of its own, that second line stops
   existing. There is no "this line is gone" item, and inventing one is B6's call if B6
   needs it, so the swallowed line settles empty — the closest this vocabulary comes to
   saying so. It takes a cursor moved back up into existing content to reach at all.

8. **Wrapped rows are joined; spacer cells are skipped; combining marks are kept.** A grid
   has a width and speech does not. A row whose last cell carries `Flags::WRAPLINE`
   continues into the next row, and extraction joins them into one logical line — otherwise
   every sentence longer than the terminal is wide arrives at the screen reader broken at
   column 80, which is a visual artifact leaking into audio. Cells flagged
   `WIDE_CHAR_SPACER` or `LEADING_WIDE_CHAR_SPACER` are the second half of a wide glyph and
   are skipped, or every CJK character doubles. `CellExtra`'s zero-width chars are appended
   after their base cell, so combining marks and accents survive. These are the three ways
   a naive `cell.c` walk produces text that is wrong in a way a sighted developer would not
   notice.

9. **Trailing blanks are trimmed per row; blank rows are kept.** A grid row is padded with
   spaces to its full width, so an untrimmed walk speaks eighty spaces after every line.
   Trailing whitespace on a row is dropped at extraction. A row that is entirely blank
   still emits its newline, because vertical spacing is structure a user navigating by
   heading and line depends on.

10. **The port is narrow, and the grid is not on it yet.** `TerminalEngine` exposes
   advancing over bytes, the current screen, resizing, and taking
   pending device replies.
   It does **not** expose the grid, the cursor, colours, or a renderable-content snapshot,
   even though `Term` has all of them and phase 2 will want them. B1 set the precedent that
   a trait is not declared ahead of its implementer; the same argument says a method is not
   declared ahead of its caller. Phase 2's renderer will add what it needs when it exists,
   and `Term` stays inside the adapter until then.

## Deliverables

### `acter-core`: `ports/driven/terminal_engine.rs`

- `TerminalEngine` (role: port), with:
  - `advance(&mut self, bytes: &[u8]) -> Vec<TerminalItem>` — one batch per read, matching
    the batch-in/batch-out shape B2's `observe` already takes.
  - `screen(&self) -> Screen` — current screen, from the emulator's own mode.
  - `resize(&mut self, columns: u16, screen_lines: u16)` — defined now because the protocol
    already defines resize and ConPTY needs dimensions at creation. It returns nothing, so
    the settlements it forces ride out at the head of the next `advance` rather than being
    dropped (amended during implementation; the alternative, giving `resize` a return value,
    would put items on a method whose caller is a window event handler).
  - `take_replies(&mut self) -> Vec<u8>` — device-query answers the caller must write back,
    drained on read so they are never sent twice.
- Documented against decisions 4 and 10: why replies exist at all, and why the grid is
  absent.

### `acter-core`: amendments

These are amendments to B2, agreed in conversation on 2026-08-17 and riding in this PR per
the process rule, not incidental edits.

- `entities/terminal_item.rs`:
  - `LineId` (role: entity/value) — an opaque monotonic newtype over `u64`, following
    `CommandId`'s shape. It lives beside the item that carries it rather than in a module
    of its own; B6 promotes it to a protocol type when the wire format learns about lines.
  - `LineRevision` (role: entity/value) — `Appended` / `Rewritten` / `Settled`, documented
    against decision 6 including which path consumes which.
  - `TerminalItem` becomes `Line { id, text, revision }`, `Marker(Osc133Marker)`, and
    `ScreenChanged(Screen)`. `Text(String)` is gone: text without identity cannot express
    revision, which is decision 6's whole point.
- `policies/boundary_tracker.rs`:
  - `BoundaryEvent::Text { region, text }` becomes `Line { region, id, text, revision }` —
    the tracker still only labels, it just carries more of what it was already carrying.
  - One arm for `ScreenChanged`, passed through as `BoundaryEvent::ScreenChanged(Screen)`.
  - **B2's central property is restated, not weakened.** "Text is never lost" was
    concatenation equality over `Text` items, which identity makes ill-typed. It becomes
    *items pass through unchanged except for the region label* — every `Line` in, in order,
    with the same id, text and revision, out. That is strictly stronger than concatenation
    equality, and it is easier to test, because it no longer has to reason about what the
    text means.
  - Its table tests and proptest strategies move to the new item shape. The region logic
    itself does not change: markers still cut, text still gets labelled.
- `lib.rs`: re-export `TerminalEngine`, `LineId`, `LineRevision`.

### `acter-term`: the adapter

- `alacritty_engine.rs` (role: adapter) — implements `TerminalEngine`. Owns the `Term`, the
  two `Processor`s, the sniffer, the reply-collecting `EventListener`, and the extraction
  cursor. A private submodule folder as its internals earn it, never a `mod.rs`.
- The sniffer, the listener, and the extractor are private modules under it; only the
  engine type is re-exported from `lib.rs`.
- The extractor owns identity: the row-to-`LineId` map, the per-row text last emitted that
  decision 5's diff compares against, the minting counter, and the forget-and-re-mint rule
  of decision 7 at a block end, a resize and a screen change.
- Marker parsing lives here: `params[0] == b"133"`, then `A` / `B` / `C` / `D`. `D`'s exit
  code is `params[2]` when it parses as an integer; any further parameters are the
  `key=value` extras some shells append (`aid=`, and similar) and are ignored. A missing or
  unparseable code becomes `CommandEnd(None)` — the case B2's decision 6 already built
  `Option<ExitCode>` for. Non-133 OSC numbers are ignored here, which is the whole reason
  the fork's hook is generic.

## Tests

Unit tests inline, ARCHITECTURE tier 1, plus proptest (already a dev-dependency in core;
new for this crate).

- **The marker cycle end to end in bytes:** the transcript from PR #17's interleaving test,
  now asserted as a `TerminalItem` stream — prompt text, `CommandStart`, the echoed command
  line, `OutputStart`, output text, `CommandEnd(Some(0))` — in order.
- **A marker split across two reads.** `ESC ] 1 3 3 ; D ;` in one `advance`, `0 BEL` in the
  next. `Processor` is stateful across calls, so this must work; it is also DESIGN's
  reliability case and B3.5 will script it deliberately.
- **Text extraction, one test per rule in decisions 5, 8 and 9:** a plain line; a line still
  incomplete at the end of a batch, then completed in the next batch with no text duplicated
  across the two; a line longer than the grid width arriving as one logical line; a CJK
  string extracted without doubled glyphs; a combining accent surviving; trailing padding
  absent; a blank line preserved.
- **Revision, the rules of decision 6, one test each:** text arriving in two reads on one
  row emits two `Appended` deltas with the same id and no repetition; a `\r` rewrite emits
  `Rewritten` with the whole line rather than a delta; `clear_line` and `erase_chars` are
  rewrites too; a line scrolling out of the screen area emits `Settled`; a block-closing
  marker settles the open lines, and those settlements appear **before** the marker in the
  returned stream; an alt-screen transition settles them too. A spinner driven for many
  reads and then stopped emits many `Rewritten` and exactly one `Settled`, which is the
  behavior the whole decision exists for. (The earlier "the newline emits `Settled`" test is
  gone with the rule it tested — see the amendment in decision 6 — and the row a newline has
  just moved onto gets a test of its own instead: it is not a line until something is
  written there.)
- **Identity, the rules of decision 7:** ids are never reused within a session; a rewrite of
  a row whose block has closed emits a **new** id rather than revising the old one; a resize
  and a screen change re-mint.
- **Multi-row in-place update:** a `docker pull`-shaped transcript that rewrites rows above
  the cursor with cursor-up revises each row under its own id, proving the change detection
  of decision 5 does not assume the last row is the only one that can change.
- **Alt-screen ordering, the case decision 2 exists for:** one batch containing text, then
  `ESC[?1049h`, then more text, yields `Line`, `ScreenChanged(Alternate)`, `Line` in that
  order, and `screen()` then reports `Alternate`. Leaving via `ESC[?1049l` reports
  `Normal`.
- **Escape sequences never reach the text.** A transcript full of SGR colour changes,
  cursor moves and mode sets extracts to exactly the visible characters — the property
  DESIGN's "never regex-strip ANSI" decision is really about.
- **Device replies:** a DSR cursor-position query produces bytes from `take_replies`, and a
  second call returns nothing.
- **Scrollback overflow is announced, not silent:** a `Term` configured with a tiny history,
  driven with more lines than it can hold in one batch, emits the speakable gap sentence
  from decision 5.

Properties. The panic property takes genuinely arbitrary bytes; the three equality
properties take generated transcripts of the fragments a terminal actually emits — printable
runs, newlines, carriage returns, tabs, backspaces, SGR, cursor up and down, column
addressing, erase-line and erase-characters. Screen swaps, resizes and block-closing markers
are deliberately outside that alphabet, because each retires the ids it settles by decision
7, so a later rewrite of the same rows becomes a *new* line — the duplication that decision
accepts on purpose, and which an equality against the final grid cannot express. Each of
those three has a table test above instead, where the behavior can be stated exactly. Blank
lines are dropped before comparing, for the swallowed-line case recorded in decision 7;
blank-line preservation is likewise table-tested.

- **Never panics on arbitrary bytes.** Moved here from B2 by B2's own decision, on the
  grounds that it belongs wherever bytes are actually parsed. This is the entry that parses
  them.
- **No line is ever lost.** For arbitrary bytes at arbitrary batch boundaries, every line
  the grid held reaches the stream, and replaying the stream — appending `Appended`,
  assigning `Rewritten` and `Settled`, all keyed by id — reconstructs exactly the text the
  grid finished with, in row order. This is B2's cardinal property re-expressed for
  identified lines, and unlike the earlier draft it is *equality*, not "at least once":
  revision removed the duplication that forced the weaker statement.
- **Every id settles at most once**, and nothing follows a `Settled` for that id. Run with
  a block-closing marker appended, since settling is exactly what one triggers.
- **Chunking-independence.** The same bytes split into different chunkings reconstruct to
  the same final text. The earlier draft could only claim this for rewrite-free transcripts,
  because flush boundaries decided what was emitted; with revision the *reconstruction* is
  chunking-independent even though the individual items are not, which is the property that
  actually matters.

## Acceptance criteria

1. `cargo test --workspace` green; `cargo clippy --workspace --all-targets` clean under
   `-D warnings`; `cargo fmt --check` clean.
2. `acter-term` depends on `alacritty_terminal` only. No `tokio`, no I/O, no transport: the
   adapter is driven entirely by byte slices handed to it, which is what makes every test
   above a plain unit test.
3. Nothing forwards to `Term`. The only `Handler` impl in this crate is the sniffer, and it
   implements exactly the three methods decision 1 names.
4. Module role declared on the first line of each new module's `//!`; the visibility ladder
   holds; only the engine type is re-exported.
5. ROADMAP's B3 entry flipped to Done, recording that the `Handler`-subset question was
   retired by DESIGN's grid decision rather than answered, and that the item stream became
   identified lines plus screen transitions.
6. DESIGN carries the line-identity decision (agreed in conversation 2026-08-17) and the
   open question it leaves: whether NVDA's browse cursor holds position when a line inside
   the buffer is mutated while the user is reviewing, which constrains the frontend but not
   this model. Recorded, not probed here — the first renderer of these items lands in B6.
7. B2's restated property is implemented and passing, and the amendment is called out in the
   PR body rather than left for a reviewer to find in the diff.
8. PR #17's dependency-wiring test (`tests/vte_unhandled_osc.rs`) stays exactly as it is. It
   asserts a property of the dependency, and it is what fails first and most legibly if the
   fork stops resolving.

## Out of scope

- **Writing device replies to the PTY.** The port exposes them; `Transport` arrives in
  B3.5, which is where bytes can be written. Until then nothing drains them in production.
- **Grid rendering, colours, cursor shape, selection, scroll-region reads** — phase 2, per
  decision 10.
- **Wiring the engine into a session.** B3.5 gives it a byte source and B6 makes it the
  app's default backend. Nothing in this PR runs at startup, so it carries **no
  accessibility checklist**: there is no user-facing surface here for either an agent or a
  human to listen to, and the first NVDA-observable behavior in this lane still arrives with
  B3.5 and B6.
- **Command-id correlation, the integration grace period, mode announcements on alt-screen
  entry.** B6's, and DESIGN's announce-versus-auto-switch question is still open.
- **Every consumer of revision.** This PR produces the facts and nothing reads them yet.
  Making speech correct under rewrites means keying B1's `UnspokenText` by line: it is a
  flat, append-only accumulator that deliberately *drops* text once the too-big verdict
  settles, so it cannot retract a line's contribution when that line is rewritten. That
  redesign, the protocol's line-aware wire format, and the frontend's id-to-node map are all
  B6's. What B3 owes them is a stream carrying enough information for a correct consumer to
  exist, which the three revision kinds do — and B3.5 is what first replays a real transcript
  through it.
- **The NVDA browse-cursor probe.** A static page mutating a rendered line, read through the
  screen-readers bridge, answers whether in-place updates are safe *during review*. It
  constrains the frontend, not this model, so it belongs with the frontend work — noted here
  so it is not lost.
- **The golden-fixture format** — B3.5's, which is the entry that replays one.
