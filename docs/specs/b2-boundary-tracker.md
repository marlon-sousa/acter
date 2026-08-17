# Spec: PR B2 — the command-block boundary tracker

Agreed in conversation 2026-08-16. Lane 2, entry B2. Delivers the state machine that
turns a stream of terminal text and OSC 133 markers into command blocks and labelled
regions — the linchpin DESIGN names for non-interactive mode.

## Why now / relation to the roadmap

- DESIGN decides command boundaries are OSC 133 and calls them "the linchpin of
  non-interactive mode": they close each response block, measure output for auto-read,
  and supply exit codes. Nothing in the repo implements them.
- B1.5's actor is *told* a command started; it never recognizes anything. This entry is
  the layer that decides that fact.
- DESIGN's reliability model (command still running, integration missing, forged
  markers) is written down and untested. Two of its three cases are decisions this
  tracker makes.

## Design decisions this spec makes

1. **Recognition is the engine's, not this entry's.** OSC 133 sequences are recognized
   by the terminal engine wrapper in `acter-term` (B3), which already runs a real
   escape-sequence parser over the byte stream; this tracker consumes what that parser
   dispatches. Decided in conversation over the alternative — a byte-level tap in
   `acter-core` scanning the raw stream ahead of the engine — because a second parser
   can disagree with the first about what is a real sequence, and there is no reason to
   own two.

   Consequence, recorded rather than discovered in review: **ARCHITECTURE's reference
   layout changes in this PR.** It lists `policies/osc133.rs (sequence recognition)`
   under `acter-core`; that file will not exist. The *state machine* stays in core, as
   the same document says; only recognition moves, into acter-term's description. The
   amendment lands here because this is the PR that makes it true.

   Consequence for scope: B2 is therefore the tracker **alone**. The "never panics on
   arbitrary bytes" property test moves to B3, where bytes are actually parsed; this
   entry keeps the equivalent property over arbitrary *marker sequences*. B2 stays ahead
   of B3 on the board anyway, because the tracker is a pure policy over a hand-written
   item stream and needs no engine to exist — and B3, landing immediately after, is its
   caller, so it does not sit uncalled the way B1's policy did.

2. **The tracker cuts; it never extracts and never filters.** Its input is an ordered
   stream of items — text, or a marker — and its output is that same text, each piece
   labelled with the region it fell in, interleaved with block lifecycle events. It
   drops nothing, rewrites nothing, and decides nothing about what a region is *for*.
   DESIGN's echo exclusion (block content is C..D only) is then a caller's one-line
   filter over labelled regions rather than a rule buried in a state machine.

3. **Regions are cut over extracted text, not over raw bytes.** This follows from
   decision 1 — with the engine upstream there are no raw byte offsets left to cut on —
   and it is the better stream to cut regardless: DESIGN's auto-read threshold already
   counts extracted text rather than bytes, "because escape sequences and prompt redraws
   would inflate the count". Carriage returns, colour changes and repaints are resolved
   by the emulator before the tracker ever sees them.

4. **Four regions, one of which is the honest-degradation case.** `Prompt` (A..B),
   `CommandLine` (B..C, the shell's echo of the submitted line), `Output` (C..D, the only
   region that is block content), and `Unstructured` — text arriving with no block
   context at all. `Unstructured` is not an error path: it is every byte of a session
   before the first marker, every byte of a session whose integration never appeared
   (DESIGN's reliability case 2), and the gap between a `D` and the next `A`. Such text
   is still rendered to the buffer; it is only never treated as a command's output. A
   screen-reader terminal that silently drops text is worse than one that admits it does
   not know where the text belongs.

5. **The tracker is id-free.** It emits `BlockStarted` / `BlockEnded`, never a
   `CommandId`. `submit_command` hands the frontend an ack id long before any marker
   arrives, so *something* must correlate the two, but that something needs the queue of
   submitted commands, which is B6's. Keeping identity out leaves this a pure function of
   its input stream. By the same argument the integration grace period stays in B6, which
   holds the `Clock`; the tracker only reports that markers were seen.

6. **Any marker meaning "we are back at the prompt" closes an open block, with an
   unknown exit code.** A prompt cannot reappear before `D` — DESIGN says exactly this,
   which is why `D` is called deterministic — so an `A` or a `B` arriving mid-block means
   the integration lied or a program forged a marker. Closing keeps the session
   speakable; ignoring would strand it in "running" until it is torn down, which is the
   worse failure for a screen reader user. The unknown exit code is represented as
   `Option<ExitCode>` in the tracker's own vocabulary, which also covers the other case
   that produces it — a well-formed `D` carrying no code. `ExitCode` is a bare `i32`
   newtype with no room for "unknown", and it is a *protocol* type: inventing a sentinel
   there would leak this into the wire format. The speakable string for "ended, exit
   status unknown" is B6's to add, when these facts first reach the frontend.

7. **Everything else nonsensical is absorbed, not rejected.** `D` with no open block is
   ignored outright (DESIGN names this case). A second `C` while a block is open is
   ignored rather than splitting the block — a program redrawing does not mean it
   restarted, the same reasoning `SessionState` already applies to alt-screen entry. A
   `B` with no preceding `A` is accepted at face value: it means the command line begins
   here, and refusing it would only lose text. There is no error type and no failure
   mode; the tracker's contract is that every input sequence produces some coherent
   output.

8. **`MarkersObserved` is latched and fires once.** It maps onto
   `SessionState::markers_observed()`, and once a session is `Integrated` that state
   never leaves — `grace_period_expired` only moves `Pending`. So a one-shot latch is
   sufficient *including* for DESIGN decision 8's recovery case: if the grace period
   expired first, the latch has not fired yet, and the first marker to arrive still
   recovers the session.

9. **Batch in, batch out.** `observe` takes an iterator of items and returns a `Vec` of
   events, because that is the shape the caller has: B3's wrapper produces one batch of
   items per read from the transport. One small allocation per read, at a cadence
   measured in tens of milliseconds, buys an API that reads the same way in tests and in
   production. A caller-supplied output buffer was considered and rejected as premature.

10. **The item types are entities, not part of a port.** `TerminalItem` and
    `Osc133Marker` describe what the engine produces, so they arguably belong to the
    `TerminalEngine` port's contract — but that port does not exist yet, and B1 set the
    precedent that a trait is not declared ahead of its implementer. They land as
    entity/value types that B3's port will reference.

## Deliverables

### `entities/osc133.rs`

- `Osc133Marker` (role: entity/value): `PromptStart` (A), `CommandStart` (B),
  `OutputStart` (C), `CommandEnd(Option<ExitCode>)` (D). Documented against DESIGN's
  marker semantics, including why `D`'s exit code is optional.

### `entities/terminal_item.rs`

- `TerminalItem` (role: entity/value): `Text(String)` or `Marker(Osc133Marker)` — one
  element of the ordered stream the terminal engine emits.

### `policies/boundary_tracker.rs`

- `Region` (Prompt / CommandLine / Output / Unstructured), `BoundaryEvent`
  (`MarkersObserved`, `BlockStarted`, `Text { region, text }`,
  `BlockEnded { exit: Option<ExitCode> }`), and `BoundaryTracker` (role: policy) with
  `observe(impl IntoIterator<Item = TerminalItem>) -> Vec<BoundaryEvent>`.
- State is two fields: the current region and the markers-seen latch. A block being open
  is exactly `region == Output`, so it is not stored twice.
- Empty text items pass through unchanged rather than being swallowed: B1.1 made an empty
  chunk *meaningful* — it must not move the quiescence deadline — so dropping one here
  would hide the case the pacing policy was built for.

## Tests

Inline table tests, plus proptest (a new dev-dependency on `acter-core`, ARCHITECTURE
test tier 1).

- **The happy cycle:** A, B, C, text, D — text labelled `Output`, exactly one
  `BlockStarted` and one `BlockEnded` carrying the exit code.
- **Echo exclusion, the mechanism DESIGN promises:** text between A and B is `Prompt`,
  text between B and C is `CommandLine`, and only C..D is `Output`.
- **Unstructured:** text before any marker, and text between `D` and the next `A`.
- **The nonsense cases, one test each:** `D` with no open block ignored; a second `C`
  ignored; `A` mid-block closes it with `None`; `B` mid-block closes it with `None`; `D`
  with no exit code closes with `None`; `B` with no preceding `A` accepted.
- **The latch:** `MarkersObserved` appears exactly once across a multi-command session,
  and not at all in a session with no markers.

Properties, over arbitrary item sequences:

- **Never panics.**
- **Text is never lost.** Concatenating the text of every emitted `Text` event equals
  concatenating the text of every input `Text` item, in order. This is the property that
  matters most: for this product, silently dropping output is the cardinal defect.
- **Blocks are balanced and never nest.** No `BlockEnded` without an open block, no
  `BlockStarted` while one is open.
- **`MarkersObserved` is emitted at most once, and only if the input held a marker.**

## Acceptance criteria

1. `cargo test --workspace` green; `cargo clippy --workspace --all-targets` clean under
   `-D warnings`; `cargo fmt --check` clean.
2. The tracker is pure: no clock, no I/O, no ports, no `tokio`. Its entire state is the
   two fields above.
3. Module role declared on the first line of each new module's `//!` comment; the
   visibility ladder holds; new types re-exported from `lib.rs`.
4. ARCHITECTURE's reference layout amended per decision 1 (recognition moves to
   acter-term; the new entity files listed), and ROADMAP's B2 entry flipped to Done with
   B3's scope sketch updated to carry recognition.

## Out of scope

- **Recognition itself** — B3, per decision 1.
- **Command-id correlation and the integration grace period** — B6, per decision 5.
- **Any wiring:** nothing calls the tracker in this PR. B3 is its first caller, landing
  next in the same lane. No user-facing behavior changes, so this PR carries no
  accessibility checklist.
- **The golden-fixture format.** B2's roadmap sketch claimed it, but the format is a
  format *of bytes* — it is what B3.5's scripted transport replays and what B5 captures
  from a real shell. With recognition moved to B3, a fixture cannot be replayed through
  anything until B3 exists. It moves to B3.5, which is the entry that actually needs to
  replay one.
