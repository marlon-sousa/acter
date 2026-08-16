# Spec: PR B1 — session state and the pacing policy

Lane 2's first entry, and the first pure-domain code in the project. Delivers the
session-scoped state machine and the auto-read/pacing policy that decides, for every
scrap of output, whether it is read aloud, announced as too big, or accumulated in
silence. Everything in this PR is deterministic: no ports, no clock, no I/O, no Tauri,
no protocol change.

## Why now / relation to the roadmap

- Roadmap lane 2, entry B1. Lane 2 has never started and convergence needs both A3
  (done) and B6, so this is the long pole.
- A3.1's NVDA run produced live pacing evidence this policy consumes: patience at
  human scale read well, four consecutive `Quiet` chunks accumulated unobtrusively,
  and back-to-back announcements were observed merging into one utterance.
- It is the entry that unblocks the announcement-ordering discussion recorded as a
  DESIGN.md open question. That discussion is *not* settled here (see out of scope) —
  but it cannot even be had until real competing streams exist, and this is the first
  code that models them.

### Deviation from the roadmap's scope sketch, agreed in conversation 2026-08-16

The sketch read "driven-port traits, session entity/state machine, auto-read/pacing
policy **against a fake clock**; table tests". Two parts of that are deliberately not
in this PR:

- **No driven-port traits.** `Transport`, `ShellAdapter`, and `TerminalEngine` are
  declared with the code that first implements them (B3, B4, B5), not guessed at here.
  Declaring a trait years before its only implementer is how a port ends up shaped for
  nobody.
- **No `Clock`, and no fake clock.** See decision 2: the policy takes elapsed time as an
  *input*, so nothing in this PR needs to ask what time it is. `Clock` lands with the
  session actor, which is the module that genuinely waits.

## Design decisions this spec makes

1. **B1 is `entities/session_state.rs` plus `policies/autoread.rs`, and nothing else.**
   Short PR per CLAUDE.md, and every line of it is pure, so the tests need no fakes at
   all. The session actor, the `Clock` port, and the mapping from pacing decisions onto
   protocol events belong to a later entry that can be designed against a policy whose
   shape is already proven.
2. **The pacing policy is pure: elapsed time in, decision plus next deadline out.**
   ARCHITECTURE defines a policy as "pure domain computation, deterministic in →
   deterministic out" and warns that "pure logic never gets a trait wrapper just for
   ceremony". Its own classifying question — *does the module do anything
   nondeterministic: I/O, time, randomness, environment?* — would reclassify a
   clock-holding policy as an adapter. So the caller supplies the timestamp and the
   policy returns what to do now and when it wants to be woken next. Table tests become
   plain data, and the fake clock never has to exist.
3. **Time is a `Duration` offset, never `Instant`.** Every timestamp crossing into the
   policy is "how long since this command started", supplied by the caller. `Instant`
   is a handle on the real clock and would drag nondeterminism into core through the
   type system; a `Duration` is a number.
4. **State is an entity threaded through pure functions, not a mutable policy object.**
   `PacingState` is an entity/value (its invariants: counters never exceed their
   configured limits, patience announces at most once per command). `autoread.rs` holds
   free functions that take the state and return a new state plus an outcome. This keeps
   the role rule honest — data with invariants is an entity, computation over it is a
   policy — and makes every test a one-line call.
5. **The accumulated text buffer stays outside the policy.** The policy is told the size
   of what is unspoken; it does not own the bytes. Holding a growing `String` for an
   endless command is buffer management, which belongs to the actor alongside the
   line-cap rendering decision DESIGN already assigns elsewhere.
6. **The babble guard is ratified**, with the count configurable like every other
   threshold: after `babble_limit` (default 3) consecutive auto-read chunks within one
   command, emit the babble outcome once and go quiet for the remainder of that command
   unless follow mode is on. It was DESIGN's last unratified pacing rule; B1 is the entry
   that implements pacing, so leaving it proposed would leave a hole exactly where the
   hardest case lives (`watch -n1`, chatty logs).
7. **The policy returns a domain decision, not a protocol type.** `PacingAction` is core
   vocabulary. It is *not* `ReadMode`, because two of its outcomes — patience and the
   babble announcement — are not read verdicts at all. Mapping decisions onto
   `SessionEvent`s is the actor's job, in a later entry. Consequence recorded rather than
   solved: the babble announcement has no protocol variant today, and will need one (or
   a widened existing one) when that mapping lands. No protocol change here.
8. **An unintegrated session can recover.** DESIGN decides a session is flagged
   unintegrated when no OSC 133 markers appear within a grace period. It does not say
   what happens if markers show up later. Decided here: they flip it back to integrated,
   and that is a state transition the session can announce. The alternative — a session
   permanently degraded despite working markers — is worse for the user and harder to
   explain than an honest "integration recovered".
9. **Session state owns session-scoped facts only.** Mode, integration status, and
   alt-screen. Per-command block transitions (prompt → command → output → exit) are the
   boundary tracker's, which is B2. The two touch but do not overlap: the boundary
   tracker reports command lifecycle events; session state answers "what kind of session
   is this, and what is on screen".

## Deliverables

All in `acter-core`. No other crate is touched.

### `entities/session_state.rs`

- `SessionState` (role: entity/value) holding `Mode` (the existing protocol value type),
  `Integration` (`Pending` | `Integrated` | `Unintegrated`), and `Screen` (`Normal` |
  `Alternate`).
- Transitions as methods returning the new state: markers observed, grace period
  expired, mode toggled, alt-screen entered, alt-screen left.
- Invariants under test: `Pending` resolves exactly once per direction; markers after
  `Unintegrated` recover to `Integrated` (decision 8); alt-screen transitions are
  idempotent (entering twice is one entry), because a program redrawing does not mean
  it re-entered.

### `entities/pacing_state.rs`

- `PacingState` (role: entity/value): consecutive auto-read count, whether patience has
  fired for this command, whether the babble guard has tripped, and the offset of the
  last output.
- `PacingConfig` (role: entity/value): `quiescence` (0.5s), `patience` (10s),
  `max_lines` (25), `max_chars` (2000), `babble_limit` (3) — DESIGN's decided defaults
  as a `Default` impl, following A3's fake-script-config precedent of numbers-as-data.

### `policies/autoread.rs`

- `measure(text) -> TextSize` — lines and chars over the extracted grid text with
  trailing whitespace trimmed, per DESIGN. Pure, and the one place measurement happens.
- `verdict(size, config) -> ReadMode` — the dual-limit threshold: over 25 lines **or**
  over 2000 chars, whichever is exceeded first.
- `on_output(state, config, size, at) -> (PacingState, PacingOutcome)`
- `on_wake(state, config, unspoken, at) -> (PacingState, PacingOutcome)` — the scheduled
  deadline fired; this is where quiescence and patience are evaluated.
- `on_command_end(state, config, unspoken) -> (PacingState, PacingOutcome)` — flush the
  remainder under the size policy.
- `PacingOutcome { action: PacingAction, wake_after: Option<Duration> }`, where
  `PacingAction` is `None` | `Flush(ReadMode)` | `StillRunning` | `OutputContinues`.
- Follow mode is an input flag: when on, thresholds and the babble guard are bypassed
  and every chunk flushes `Auto`.

### `lib.rs`

Facade re-exports for the types a later actor will name.

### Tests

Table tests, inline per convention, no fakes anywhere:

- **Thresholds:** at, one under, and one over each limit; the dual limit exercised in
  both directions (many short lines under the char cap; few long lines over it);
  trailing-whitespace trimming changing a verdict.
- **Quiescence:** output then silence past the window flushes; output arriving inside
  the window defers the flush and extends the deadline.
- **Patience:** continuous output with no quiescent gap for the whole window fires once
  and only once; a session that goes quiescent before the window never fires it; a
  command whose output resumes after patience does not re-fire.
- **Babble guard:** the Nth consecutive auto-read trips it; the outcome is emitted once;
  subsequent chunks in that command are silent; a new command resets the count; follow
  mode suppresses the guard entirely.
- **Command end:** the remainder is flushed under the size policy; a command ending
  inside a quiescence window still flushes; ending with nothing unspoken emits no action.
- **Session state:** every transition in decision 8 and 9, including the recovery case
  and alt-screen idempotence.

## Acceptance criteria

1. `cargo test --workspace` green, including the new table tests.
2. Nothing in this PR imports `std::time::Instant`, `SystemTime`, `tokio`, `tauri`, or
   touches the filesystem, the environment, or randomness. The policy is a function.
3. No protocol change: `ui/src/protocol.ts` is byte-identical and the A2 drift guard is
   green without regeneration.
4. Every DESIGN-decided number appears exactly once, in `PacingConfig::default`.
5. Module role declared on the first line of each new module's `//!` comment, and the
   visibility ladder holds (`pub` only on items re-exported from `lib.rs`).
6. No manual accessibility checklist: this PR has no user-facing surface. The strings its
   decisions will eventually produce are pinned in the frontend and NVDA-validated
   already (A3, A3.1).

## Out of scope

- The session actor, the `Clock` port, and mapping `PacingAction` onto `SessionEvent`s —
  the next lane-2 entry.
- `Transport`, `ShellAdapter`, `TerminalEngine` — declared with their implementers
  (B3/B4/B5).
- OSC 133 recognition and the command-boundary tracker — B2.
- The protocol variant the babble announcement will need, and its pinned string
  (decision 7).
- **The announcement-ordering question.** A3.1 showed two announcements appended within
  one tick are spoken as a single concatenated utterance, and DESIGN records that as an
  open question. Whether the fix belongs in this policy (by refusing to emit two
  announcements too close together) or in the view is exactly what that discussion must
  settle, and it should be settled with real streams in hand — which this PR creates but
  does not yet run. Deliberately left open.
- The Profile domain and per-session overrides: `PacingConfig` is plain data here, the
  same scope line A3 drew around its fake script config.
- Line-cap rendering for endless output, and the unspoken-text buffer itself
  (decision 5).

## Implementation notes (amendment, added during implementation)

Two plumbing decisions the deliverables section left implicit, recorded here rather
than left to silently diverge:

- **`on_output` takes an added `follow_mode: bool` parameter**, not shown in the
  deliverables' signature. Follow mode's bypass ("every chunk flushes `Auto`,
  thresholds and the babble guard bypassed") has to happen synchronously at the
  moment a chunk arrives, since under follow mode `on_output` never schedules a wake
  (`wake_after: None`) — there is no later `on_wake` call for it to happen in.
  `on_wake` and `on_command_end` keep the literal signatures: `on_wake` is simply
  never scheduled while follow mode is active (nothing to bypass), and
  `on_command_end` sees an empty buffer by construction under follow mode (decision
  5 — the actor keeps the unspoken buffer empty because every chunk was already
  flushed), so no parameter was needed there either.
- **Patience is anchored to command start, not reset by quiescence gaps.** DESIGN's
  "no quiescent gap for the whole window" reads as literally requiring an
  uninterrupted stretch, which would need a fifth `PacingState` field (a
  "continuous-since" timestamp) beyond the four the deliverables section lists.
  Instead, patience fires once elapsed time since command start reaches
  `PacingConfig::patience` and no quiescence-triggered flush has happened to occupy
  that exact wake — a command that goes quiescent and ends well before the patience
  window simply never reaches it, which satisfies the spec's own patience test case
  ("a session that goes quiescent before the window never fires it") without the
  extra field. If a real command later needs the strict "gap resets the window"
  behavior, that is a one-field addition, not a rearchitecture.

## Definition of done

Merged with green CI. `acter-core` contains a session state machine and a pacing policy
that are pure, exhaustively table-tested, and free of any notion of what time it is.
ROADMAP.md's B1 entry flipped to Done with this spec's path, and the next lane-2 entry
(session actor plus `Clock`) added ahead of B2 if the conversation agrees it belongs
there.
