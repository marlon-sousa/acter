# V3 — the output carries the far end's colours

Lane 5, the look; roadmap entry 54. It ships as two PRs, V3 and V3.1. The spec lands with V3.

## What is true today

Every output row reaches the frontend as plain text. A sighted person misses the red error, the
coloured `ls` listing and the green and red of `git diff` before anything else.

Read from the code on 2026-09-23:

- **The engine has colour and drops it.** `alacritty_terminal` 0.26.0's `Cell` holds
  `c`, `fg`, `bg`, `flags` and `extra`. `fg` and `bg` are `vte::ansi::Color`: `Named`, `Indexed(u8)`
  or `Spec(Rgb)`. `flags` carries BOLD, DIM, ITALIC, UNDERLINE (and its variants), INVERSE,
  HIDDEN and STRIKEOUT. The extractor's `View::read_row` (extractor.rs) reads only `c`, the
  zero-width characters, and the flags for wide-character spacers and wrapping.
- **Text is the only record of a line.** `TerminalItem::Line { id, text, revision }`, and
  `Tracked { id, emitted }` compares text only. A change that is colour alone is not a change
  today.
- **The same text feeds many consumers.** In `Pump::line` (session.rs), one row's text feeds:
  - the prompt text and `PromptDrawn`, which the frontend speaks;
  - echo detection;
  - speech (`due`, `ReadAloud`, `TooBig`);
  - far-end-line row diffs and `FarEndLine`;
  - `Held`, which concatenates appended text;
  - the session actor's coalescing of `Appended`.
  `SessionEvent::Output { command_id, line, revision, text }` is documented as "what to put in
  the buffer, never what to say".
- **TypeScript types are generated** with specta, by
  `cargo test -p acter-app --test protocol_bindings`, which writes `ui/src/protocol.ts`. CI fails
  when the generated file differs from the committed one.
- **The frontend sets text only:** `BufferDom.applyLine` uses `textContent`.

Campbell, the palette DESIGN's look section names, against the session background
`#0C0C0C`, measured 2026-09-23:
- **Below 4.5:1:** red 3.2, blue 2.4, magenta 2.4, bright black 4.3 and bright magenta 3.2.
- **At or above 4.5:1:** green 5.7, yellow 7.5, cyan 6.1, white 12.2, bright red 5.1, bright
  green 8.5, bright yellow 16.9, bright blue 5.0, bright cyan 11.3 and bright white 17.5.

## Decisions

1. **Colour is metadata on the text, and nothing speaks it** (the user's direction,
   2026-09-23). The text of every line is exactly what it is today. Colour travels beside it
   as runs, so every consumer that speaks, compares or diffs text is unchanged and never sees a
   run. Whether a screen reader user should ever hear colour is decided later, with the
   colour already there to work from.
2. **A run is a span of the line's text with one style.** `StyleRun { start, len, style }`:
   - `start` and `len` are in UTF-16 code units, because the frontend slices JavaScript
     strings;
   - `Style` holds `fg` and `bg` as `Option<Colour>`, plus `bold`, `dim`, `italic`,
     `underline`, `inverse` and `strike`;
   - `Colour` is `Named(0..15)`, `Indexed(16..255)` or `Rgb { r, g, b }`;
   - the terminal's own default foreground and background are `None`;
   - `HIDDEN` text is sent as it is with no run, because the grid already holds what the far
     end printed.
   - Runs with the default style are left out, so a line with no colour carries an empty list.
     Cell by cell, zero-width characters belong to their base character's run.
3. **The runs follow the revisions.**
   - `Appended` carries runs relative to the delta.
   - `Rewritten` and `Settled` carry runs for the whole line.
   - Where the pipeline joins appended text (`Held`, the actor's coalescing), it shifts the
     later runs by the earlier text's length.
4. **A change of colour alone is a `Rewritten` of the same text.** The extractor's record gains
   the runs, so a row that changes only in colour is emitted as `Rewritten`. `Rewritten` is
   never spoken. For every consumer except the buffer, a `Rewritten` whose text equals the
   row's current text is not a change: far-end row diffs and echo detection ignore it. So
   PSReadLine's colour-only menu highlight moves on screen and causes nothing else.
5. **The protocol carries the runs to the buffer and nowhere else.**
   `SessionEvent::Output` gains `runs: Vec<StyleRun>`. `PromptDrawn`, `FarEndLine` and
   announcements do not change.
6. **V3.1 draws them.** Each row is rendered as text nodes and `span`s. A row with no runs
   stays a single text node, as today.
   - Named colours use Campbell as CSS custom properties.
   - Indexed colours use the xterm 256-colour table.
   - `Rgb` is used as given.
   - `inverse` swaps foreground and background; `bold`, `italic`, `underline`, `strike` and
     `dim` map to their CSS equivalents.
7. **Every drawn colour is kept readable.** When a run's foreground is below 4.5:1 against the
   background it is drawn on, V3.1 raises its lightness until it reaches 4.5:1, keeping its hue.
   Windows Terminal offers the same adjustment as a setting. For Acter, a low-vision user's
   ability to read the red error comes before the exact shade.
8. **Windows high contrast still wins.** Spans take system colours under `forced-colors`, like
   everything else.

## What an NVDA user may notice

The text NVDA reads is unchanged. But colour becomes real formatting in the page. So a user who
has turned on NVDA's "report colour" (Document formatting settings; it is off by default)
will hear colour names as they read. That is the reader's own setting doing what the user asked
it to do, and Acter does not suppress it.

## Files touched

**V3:**
- `crates/acter-core/src/entities/terminal_item.rs`, plus new style entities in acter-core;
- `crates/acter-term/src/alacritty_engine/extractor.rs`;
- the boundary tracker;
- `crates/acter-core/src/services/session.rs`;
- `crates/acter-core/src/controllers/session_actor.rs`;
- `crates/acter-core/src/entities/protocol_events.rs`;
- `crates/acter-app/tests/protocol_bindings.rs` and the regenerated `ui/src/protocol.ts`;
- test helpers in each crate.

The frontend accepts `runs` and ignores them.

**V3.1:**
- `ui/src/adapters/buffer.ts`, its port and its tests;
- `ui/src/styles.css`;
- a small colour policy module (palette, 256-colour table, contrast adjustment) with its own
  tests.

## Acceptance criteria

**V3:**
1. Extractor tests pin runs for:
   - a red word in plain text;
   - a colour change in the middle of a line;
   - `Appended` deltas with offsets;
   - a colour-only rewrite (emitted as `Rewritten`, with the same text);
   - wide characters;
   - text with zero-width characters.
2. Pipeline tests:
   - joined appends shift runs correctly;
   - a colour-only `Rewritten` produces no speech, no far-end row change and no echo;
   - `SessionEvent::Output` carries the runs.
3. Every existing Rust and UI test passes. Tests change only where a helper gains a `runs`
   argument.
4. `ui/src/protocol.ts` is regenerated and committed.

**V3.1:**
1. Colour policy tests:
   - each Campbell colour below 4.5:1 is adjusted to 4.5:1 or above;
   - colours already above the threshold are unchanged;
   - the 256-colour table matches xterm's values.
2. Buffer tests: runs become spans with the right text and styles; `Appended` spans join a row
   correctly; a row without runs stays one text node.
3. Screenshots against real shells:
   - PowerShell's red error;
   - `ls --color` under WSL;
   - `git diff` in a scratch repository;
   - the same under emulated forced colours.
4. NVDA 2026.1.1 through the bridge, `user` persona, default settings: the same coloured
   output reads line for line the same as with colour removed.
