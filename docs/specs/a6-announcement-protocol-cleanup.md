# Spec: PR A6 — announcement protocol cleanup

Retires the three ways the protocol still had of saying something a
`SessionEvent::Announce` already says: the `read_mode` field on `Output` and
`CommandFinished`, the `CommandStillRunning` event, and the
failure-via-`CommandFinished.exit_code` path. After this PR, `Announce` is the only
way the backend *asks for speech* — see decision 6 for why that is a narrower claim
than "the only event that causes speech", and which events keep causing it.

## Why now

A2's exhaustiveness guard fails `tsc` until every `SessionEvent` variant is handled,
so a protocol *addition* cannot be split across two PRs while a *removal* can wait.
B1.5 added `Announce` additively and left the shapes it supersedes in place; this
entry is the debt that decision recorded, and B6 is what makes it collectable.

B6 changed the size of the job in both directions. It removed half the work — the
plan written in ROADMAP entry 10 included migrating `FakeSessionService` onto
`Announce`, and B6 deleted that service outright, so there is exactly one session
service now, it emits `Announce` already, and nothing scripts a verdict anywhere in
the tree. It also made the remaining half safe to do, because every producer of both
fields now sets `ReadMode::Quiet` unconditionally.

That last fact is the one worth stating plainly, because it is what makes this a
cleanup rather than a behavior change: **the code being deleted is already
unreachable in the shipped product.** `Auto` and `TooBig` reach the frontend from no
producer, so the branches on them in `app.ts` never run against the real backend.
What speaks today is `Announce`, and only `Announce`.

## What the survey found

Facts this spec is built on, established by reading main at `6d89b89`:

- Every emitter sets `read_mode: ReadMode::Quiet`: `session_actor.rs` lines 284, 382
  and 613, and three sites in `services/session.rs` (974, 1134, 1182). No production
  path constructs `Auto` or `TooBig` on the wire.
- `Announce` already covers both live verdicts exactly, including the bookkeeping:
  `ReadAloud` is the `Auto` branch, and `TooBig` both announces *and* performs the
  `this.tooBig.add(commandId)` that arms the completion beep — the same add the
  `Output` branch does.
- `CommandStillRunning` and `Announcement::StillRunning` map to the identical pinned
  string (`patienceMessage`) in `app.ts`.
- The failure path is already dead frontend-side: `app.ts` says in a comment that it
  deliberately says nothing about the exit code, because B6's manual pass found that
  announcing it there said it twice and said it first, ahead of the line it was about.
- No e2e spec references any of the three. The concentration of change is
  `ui/test/controllers/app.test.ts`, where roughly thirty emits carry `read_mode`.

## Design decisions this spec makes

**1. `ReadMode` loses its place on the wire, not its place in the domain.**
The roadmap called the field vestigial, and the field is; the *type* is not.
`verdict()` returns `ReadMode`, `PacingAction::Flush(ReadMode)` carries it,
`unspoken_text.rs` tests against it, and `policies/autoread.rs` uses it about forty
times. So this PR removes the field from two events and demotes the type: it moves to
its own entity/value module, `crates/acter-core/src/entities/read_mode.rs`, becomes
`pub(crate)` rather than re-exported, loses its `TS` derive and its
`protocol_bindings.rs` registration, and disappears from `ui/src/protocol.ts`.

Folding it into `policies/autoread.rs` beside its only producer was considered and
rejected: the module role rule is literal, a policy module does not own a value type,
and the value already outlives the policy in `unspoken_text.rs`. Leaving it in
`protocol_common.rs` was rejected for the reason this PR exists — a module named for
the protocol must not keep a type the protocol no longer carries.

**2. `CommandFinished` keeps only its `command_id`.**
Both fields go. `Announce { Failed { exit_code } }` becomes the single carrier of an
exit code, and `CommandFinished { command_id }` ends up the same shape as
`CommandInterrupted`, which is honest: both say a block closed, and neither says
anything about how.

The consequence to accept knowingly is that **a successful command's exit code is no
longer on the wire at all**, because success sends no announcement. That is not a
loss today — nothing in `ui/` reads the field, and the frontend's own comment says it
must not speak it — but it is the thing to revisit if a later entry wants an "exit
code" query or the code in a block's accessible name. Such an entry adds a field or
an event deliberately; it does not get to rely on one nobody was reading.

**3. `CommandStillRunning` is deleted, `IntegrationUnavailable` is not.**
The patience event is a pure duplicate of `Announcement::StillRunning` down to the
spoken string, so it goes. The neighbouring session-scoped events stay exactly as
they are: B6 decision 11 already reasoned that `IntegrationUnavailable` is not an
`Announce` because it carries no `command_id` — it fires at session start, before any
command exists — and `AltScreenEntered` / `AltScreenLeft` have the same shape for the
same reason. This PR does not reopen that.

**4. Render-before-announce becomes an ordering guarantee between two events.**
A5.2 pinned the invariant inside the controller: append to the buffer before
announcing, so spoken text is always already in the buffer. With `read_mode` gone,
`Output` no longer announces anything, and the invariant now spans two events — the
backend must emit `Output` before the `Announce` that is about it. It holds on main
already; what this PR adds is a test that says so, at the service level, so that a
future reordering fails a test instead of a listener.

**5. Nothing user-facing changes, and the checklist exists to confirm it.**
No pinned string is added, removed or reworded; no announcement is added or lost.
The manual pass confirms exactly that — that a change deleting the code a listener's
speech *appears* to come from changes nothing a listener hears.

**6. The rule this cleanup applies, and the claim it does not make.**
It would be wrong to say that after this PR the frontend has one way to be told to
speak. `CommandInterrupted`, `AltScreenEntered`, `AltScreenLeft` and
`IntegrationUnavailable` all still cause speech, and are all meant to. The real
distinction is narrower and worth stating, because it is what decides which shapes
die here and which stay:

- `Announce` is the backend **asking for speech** about a command. Its `command_id`
  is mandatory and its payload is about that command.
- Every other variant is a **fact about session or block state**. The frontend
  decides whether that fact is worth voicing, in a pinned string it owns, and often
  does bookkeeping on it as well.

So a non-`Announce` variant is redundant exactly when an `Announce` already fires on
the same trigger and yields the same string. `CommandStillRunning` meets that test —
same trigger, same `patienceMessage` — and the `read_mode` fields meet it for
`ReadAloud` and `TooBig`. That is the whole of what A6 deletes.

The session-scoped events do **not** meet it, and this PR deliberately leaves them
alone. They have no `Announce` counterpart, so retiring them would mean *inventing*
`Announcement` variants — an addition, which the `tsc` exhaustiveness guard forbids
splitting across PRs, landing inside a PR whose entire purpose is deletion. Worse,
they carry no `command_id`: folding them in would force `Announce::command_id` to
become optional, weakening the type for every announcement to accommodate three that
are genuinely not about a command. B6 decision 11 reached this conclusion already;
A6 confirms it rather than reopening it.

`CommandInterrupted` is the one that superficially qualifies, since it does carry a
`command_id`. It stays because it is not purely speech: the frontend deletes the
command from `tooBig` and `openBlocks` on it. Converting it to an announcement would
discard the block-closed fact in order to deduplicate a string.

## Deliverables

Backend, `crates/acter-core`:

- `entities/read_mode.rs` — new entity/value module holding `ReadMode`, `pub(crate)`,
  no `TS` derive. Removed from `protocol_common.rs`, from the `entities.rs` re-export
  list and from the public facade in `lib.rs`.
- `entities/protocol_events.rs` — `Output` loses `read_mode`; `CommandFinished` loses
  both `read_mode` and `exit_code`; `SessionEvent::CommandStillRunning` is deleted.
  The `every_variant()` fixture and the JSON round-trip tests follow.
- `controllers/session_actor.rs` — the three `read_mode: ReadMode::Quiet` sites and
  the `exit_code` site drop their fields. The `match` on the pacing verdict at lines
  341–358 is untouched: that is the policy driving `Announce`, not the wire field.
- `services/session.rs` — the three emit sites and every test assertion naming the
  removed fields.

Bindings and frontend:

- `crates/acter-app/tests/protocol_bindings.rs` — drop the `ReadMode` registration
  and import.
- `ui/src/protocol.ts` — regenerated: no `ReadMode`, no `CommandStillRunning`, the
  two event shapes narrowed.
- `ui/src/controllers/app.ts` — the `Output` handler appends and nothing else; the
  `CommandFinished` handler keeps the beep-on-too-big bookkeeping and the block
  cleanup but loses its `read_mode` branch; the `CommandStillRunning` case is
  deleted. `handleAnnouncement` is unchanged — it was already carrying this.
- `ui/test/controllers/app.test.ts`, `ui/test/protocol.test.ts` and
  `crates/acter-transports/tests/pipeline.rs` — emits and fixtures migrated to
  `Output` + `Announce` pairs.

## Tests

- The existing suites, migrated rather than deleted: an announcement asserted through
  `read_mode: 'Auto'` is asserted through `Announce { ReadAloud }`, so coverage of
  *what is spoken* does not shrink along with the field.
- A service-level test pinning decision 4: for a chunk that gets read aloud, the
  `Output` event precedes its `Announce` in the emitted sequence.
- A test pinning that the completion beep is armed by `Announce { TooBig }` alone,
  now that no `CommandFinished.read_mode` can arm it.
- The `tsc` exhaustiveness guard is the removal's own proof: a `SessionEvent` case
  for a variant that no longer exists fails the build.

## Acceptance criteria

- `SessionEvent` carries no `read_mode` anywhere, no `CommandStillRunning` variant,
  and no `exit_code` outside `Announcement::Failed`.
- `ReadMode` is absent from `ui/src/protocol.ts` and from the generated bindings, and
  still drives the autoread policy unchanged in Rust.
- `cargo test`, `cargo clippy`, the `ui` unit suite and the e2e suite are green, with
  no changes to any e2e spec.
- No pinned user-facing string differs from main, by diff.

## Manual accessibility checklist (PR body)

Confirming that nothing a listener hears has changed. Agent-observable through the
screen-readers MCP bridge, as the `user` persona, except where noted.

- A short command's output is read aloud once, in the same words as on main.
- A too-big output announces its line count, and the completion beep still fires
  (**human-only**: the bridge captures speech and braille, not audio).
- A long-running command speaks the patience string once, after the patience window.
- A failing command speaks the failure verdict once, after the output it is about.
- A stopped command speaks `command stopped` and does not beep.

## Out of scope

- The correlation drift filed as B6.1: this PR removes an event, not the id queue.
- A3.2's `Ctrl+C` surface, which is the entry after this one in lane 1.
- Any new UX for `TitleChanged` or `ConnectionChanged`; they stay handled-and-silent.
- Re-opening B6 decision 11 about `IntegrationUnavailable`.

## Definition of done

The three superseded shapes are gone from protocol, actor and frontend; `Announce` is
the only way the backend asks for speech; every suite is green; and the ROADMAP entry
for A6 is flipped to Done with this spec's path, in this PR.
