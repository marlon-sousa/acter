# Spec: PR B6 — the real `SessionService`

Agreed in conversation 2026-08-19. Lane 2, entry B6. Delivers `acter-core`'s first
service: the component that owns a session end to end — transport, engine, tracker and
actor wired together, command-id correlation, the integration grace period, and the
interrupt surface — and switches the app's default backend from the A3 event-level fake
to the byte path.

## Why now / relation to the roadmap

- B3.5 put a real transcript through the real engine, tracker, policy and actor, but did
  it in a test file with glue its own spec named as this entry's to promote (B3.5
  decision 8). Nothing in the shipped app runs any of it: `FakeSessionService` is still
  the default backend, so every manual NVDA session still validates the frontend against
  a hand-written event stream.
- This is the entry where the ten scenarios become audible against the real policy.
  B3.5 decision 9 states the consequence plainly: A3's fake scripted every verdict and
  the scripted transport scripts none, so `TooBig`, the patience announcement, the babble
  guard and the auto-read decision are computed here for the first time in a session a
  human can hear.
- It is convergence made user-facing, months before ConPTY exists. B4 and B5 then change
  what is at the far end of a seam that already works, rather than being the first
  moment the pipeline runs at all.
- Two pieces of the domain are currently dead code with no production caller:
  `Integration::{Pending, Integrated, Unintegrated}` with its two transitions, and
  `SessionEvent::CommandInterrupted`, whose only producer is the fake this entry deletes.
  Both come alive here.

## Design decisions this spec makes

1. **Placement: `acter-core/src/services/session.rs`, plus a `services.rs` facade.**
   `acter-core`'s first service module, at the path ARCHITECTURE already reserves for it.
   Service and not controller by the module role rule: `SessionActor` remains the
   per-session loop that owns pacing and session state, and this component owns the
   wiring, the correlation, and the lifetime of both. Deleting the actor would lose
   business behavior; deleting this would lose connectivity.

   It depends only on ports — `Transport`, `TerminalEngine`, `Clock`, `EventSink` — so it
   names no adapter and `acter-core` gains no dependency on `acter-term` or
   `acter-transports`. The container picks the implementations.

2. **Two tasks per session, one owner each.**

   The actor task is `SessionActor::run(inputs)`, unchanged — it already exists and
   already selects over its input channel and its two timers.

   The **pump task** owns `Box<dyn Transport>`, `Box<dyn TerminalEngine>`,
   `BoundaryTracker` and the correlation queue, and selects over two channels: the bytes
   the transport pushes, and the requests `SessionApi` submits.

   `Transport` is `Send` and not `Sync`, and every method takes `&mut self`, so it has
   exactly one owner by construction. Making the pump that owner is what lets
   `take_replies` be written back with no lock and keeps writes ordered against reads —
   a device-query answer must not overtake a submitted line. A `Mutex<dyn Transport>`
   shared between the router thread and a reader task was rejected for exactly that
   ordering reason, not for contention.

   `SessionApi`'s methods stay synchronous (the trait is sync and dyn-compatible by
   ARCHITECTURE's rule, and an invoke never waits on the shell), so they `try_send` onto
   the request channel and return.

3. **Correlation: ids minted at submission, claimed at `BlockStarted`.**

   `submit_command` mints the next `CommandId` from an `AtomicU32` starting at 1 (so 0
   never appears as a real command, as the A3 fake also arranged), returns `SubmitAck`
   immediately, and sends the id with the line. The pump keeps a FIFO of submitted ids
   and pops the front at `BoundaryEvent::BlockStarted`, which is OSC 133 C and therefore
   always after the echo.

   Two edges, answered here rather than discovered:

   - **A block opens with an empty queue** — the shell's own activity, or a forged `C`.
     Mint a fresh id and treat it as a real command. A block genuinely opened, and its
     output has to go somewhere; dropping it loses text, which is this product's cardinal
     defect (B2's property test exists for the same reason).
   - **A submission never opens a block** — its id stays queued and the next block claims
     it. Phase 1's shell is serial, so the queue is short. This is the one place
     mis-attribution can hide, and it is accepted knowingly rather than papered over: a
     timeout that retires an unclaimed id would guess, and guessing wrong is what DESIGN's
     reliability model exists to avoid.

4. **The frontend sends the keystroke, not the meaning.**

   `SessionApi` gains one method carrying what was pressed:

   ```rust
   fn send_key(&self, session: SessionId, key: KeyPress) -> KeyAck;
   ```

   The frontend reports `Ctrl+C`; the domain decides that `Ctrl+C` means interrupt. Three
   things stay separated that A3.1's framing had tangled — the **key**, the **intent**,
   and the **bytes or syscall** — but the cut between frontend and domain falls at the
   first boundary, not the second.

   Four reasons, in ascending order of force:

   - **Bindings are configuration, and configuration is the backend's.** DESIGN Decided
     that all keybindings are configurable and global. If the frontend mapped key to
     intent, the binding table would live in the frontend, away from the profile and
     settings machinery that will own it.
   - **The frontend should not know the vocabulary of meanings.** Adding EOF or suspend
     later becomes a domain change plus a config entry, with no frontend release.
   - **The pass-through key already requires this shape.** `Ctrl+Shift+Space` is a Decided
     binding meaning "send the next keystroke literally to the app". That is not
     expressible in a vocabulary of intents — it is *by definition* a keystroke the
     frontend must describe without interpreting. So a keystroke-shaped message is not a
     choice this spec makes; it is already implied by a Decided binding, and an
     intent-shaped port would have needed a second, parallel one beside it.
   - **Phase 2 is the same protocol.** Keystroke map layer 3 says interactive mode passes
     everything that is not layer 1 to the app. If the wire is already keystroke-shaped,
     interactive mode extends this path instead of introducing a second one.

   **What the frontend still decides, and therefore never sends.** Layer 1
   (`Ctrl+Shift+letter`) is Acter's own commands and is consumed locally — it is never
   passed to the app and never reaches this method. Layer 2's native affordances are
   likewise consumed: `Ctrl+C` **with a selection** copies, which is an edit-field
   behavior that stays in the frontend entirely. Only `Ctrl+C` **without** a selection is
   unconsumed, and that is what gets forwarded. So this is not "every keypress crosses
   the IPC boundary" — it is "the keys the frontend did not claim", which in phase 1 is a
   very short list.

   Encoding the interrupt byte in the frontend remains rejected for the reason in decision
   5. This also resolves A3.2's blocked question in the direction DESIGN's **Decided**
   edit-field ownership already points — the field is 100% local and the shell sees no
   bytes until Enter, so an interrupt is never "a key that reaches the pty"; it is a
   report to the session about a key the frontend chose not to handle.

5. **`Transport::interrupt` is a method, because over SSH an interrupt is not a write.**

   ```rust
   fn interrupt(&mut self) -> Result<(), TransportError>;
   ```

   Over a local ConPTY an interrupt may be a control byte in the data stream; over SSH it
   is a channel request outside it. So the service cannot compute bytes and call `write`.
   This is the same reasoning that already put `resize` on the trait as its own method
   rather than as bytes — its doc comment's "both have to happen, and only one of them is
   I/O" generalizes.

   `ScriptedTransport` implements it by feeding its existing interrupting-rule path, which
   `default_transcript.json` already exercises through its interrupt-byte rule (the rule
   matching the byte `0x03`), so the port ships with a working implementer in the same PR.

   **Updated after B3.6:** that path now lives behind `FakeShell`. Which submissions
   interrupt is the far end's knowledge, and the pipe only asks — so the implementation is
   the pipe's own vocabulary rather than anything that re-matches a rule: hand the byte the
   shell's line discipline already recognizes to the emission loop, exactly as a write of
   `0x03` arrives today. `ScriptedTransport` gains no knowledge of what an interrupt *is*,
   which is the property B3.6 exists to protect.

6. **Both vocabularies ship with one member, and the map between them is a policy.**

   ```rust
   pub struct KeyPress { pub key: Key, pub ctrl: bool, pub shift: bool, pub alt: bool }
   pub enum Key { Char(char) }
   ```

   `Key` gets named variants (Tab, Escape, arrows, function keys) when an entry needs
   them — Tab with A4's completion, the rest with phase 2's pass-through — and not
   before. Shipping the full keyboard now would mean a dozen variants with no consumer,
   which is the shape B1 refused to create and B3.5 decision 10 restated; the same rule
   that keeps `Eof` out below cannot be waived here.

   The intent side is `SessionIntent::Interrupt` and nothing else — a domain type, not a
   wire type. EOF is the obvious second member and is deliberately **not** here: by
   DESIGN's own transport-versus-shell criterion ("bash-over-SSH and bash-over-WSL share
   the bash adapter on different transports"), EOF is shell knowledge — PowerShell on
   ConPTY wants Ctrl+Z, bash-over-WSL wants `0x04`, same transport and different answers —
   and `ShellAdapter` does not exist yet (`acter-shells` is a five-line stub). Filed as
   B5's. Interrupt, by the same criterion, is transport knowledge: fix the shell to bash
   and interrupting over WSL and over SSH are different mechanisms.

   **The map itself is `policies/keybindings.rs`** — a pure function from `KeyPress` to
   `Option<SessionIntent>`, which is exactly the module role a decision table has. It
   ships with one entry, `Ctrl+C` to `Interrupt`; returning `None` for everything else is
   the honest answer for a key nothing is bound to.

   **Configurability is deliberately deferred.** DESIGN Decided that bindings are
   configurable and global, and this entry does not build that: profiles and the
   configuration screen are post-convergence, and inventing a settings store here would be
   the largest thing in the PR and the least tested. What matters now is that the *seam*
   is in the right place — once the table is a policy behind the port, making it
   configurable is replacing a constant with a loaded value, and no frontend or protocol
   change is involved. That is the whole benefit of decision 4, banked without paying for
   the config machinery yet.

7. **No `command_id` on a keystroke, and an ack that says what became of it.**

   DESIGN says Ctrl+C "interrupts the running command"; the service knows which one, and
   a frontend-supplied id can only be stale, because the command may have ended between
   the keypress and the invoke. So the service targets whatever is running.

   `KeyAck` answers two questions the frontend cannot answer itself: whether the key was
   bound to anything at all, and — when it was — whether there was anything to act on.
   A3.1 decision 6 noted that the typed `stop` has no honest way to say "nothing to stop"
   and that `Ctrl+C`, "which has an ack to report it", would. This is that ack, and the
   keystroke protocol adds the second case: a key the domain has no binding for must be
   distinguishable from a key that was bound and found nothing running, because those are
   different things to say to a listener. What the frontend says for each is A3.2's.

8. **Interrupted is distinguished from finished by what the service wrote, not by the
   exit code.**

   `BoundaryEvent::BlockEnded { exit: None }` is overloaded: B2 documents it as *either*
   a bare `D` *or* a prompt reappearing mid-block. B3.5's glue mapped `None` to
   `ExitCode(0)` with a comment naming the right answer as this entry's. Left alone, a
   Ctrl+C would be announced as "finished, exit code 0" — a wrong announcement, and
   precisely what A3.1 decision 4 created `CommandInterrupted` to avoid.

   The tracker cannot fix this; a bare `D` is a bare `D`. The service can, because it
   knows what it just asked the transport to do: it records that an interrupt is
   outstanding for the running command, and a block closing with `exit: None` while one is
   outstanding emits `CommandInterrupted` instead of `CommandFinished`. That is
   correlation, which is this entry's by definition, and it gives
   `SessionEvent::CommandInterrupted` a producer again rather than orphaning it when the
   fake is deleted.

   A block closing with `exit: None` and **no** outstanding interrupt still ends the
   command — stranding a session in "running" remains the one answer that is certainly
   wrong (B2) — and reports `ExitCode(0)`, unchanged from the glue.

   **Amended during implementation: this needs a third `SessionInput`.** The spec says
   "the service emits `CommandInterrupted`", and the service emits nothing directly — the
   actor owns the sink, and it has to, because it is holding unflushed rendered text that
   must reach the buffer *before* anything is said about the command. Sending the event
   past the actor would put the announcement ahead of the text it is about, which is the
   one ordering invariant DESIGN states in the frontend's terms. So `SessionInput` gains
   `CommandInterrupted { command_id }` beside decision 9's two, and the actor closes the
   command the same way it closes a finished one — the policy's last word on the
   remainder, the render flush, then the terminal event — with no `Failed`, because the
   exit code of a process the user stopped carries nothing worth announcing.

   Adding a variant to `SessionInput::CommandEnded` instead (`exit_code: Option<..>`) was
   rejected on this decision's own grounds: it would re-overload absence, which is exactly
   the ambiguity this decision exists to remove.

9. **The integration grace period becomes real, and the actor keeps `SessionState`.**

   `PacingConfig` gains `integration_grace`. `SessionInput` gains `MarkersObserved` and
   `GracePeriodExpired`; the pump forwards `BoundaryEvent::MarkersObserved` as the first,
   and the service arms a `Clock` timer at session start for the second. The actor keeps
   ownership of `SessionState`, which it already holds and already uses for alt-screen —
   it is the component whose behavior must change, so the state belongs where the
   behavior is.

   **Default: 5 seconds.** DESIGN gives no number. The grace period only has to cover
   shell startup, since the injected snippet emits markers on the first prompt — but
   PowerShell profile loading routinely takes seconds, and the asymmetry is sharp: a
   false `Unintegrated` degrades every command in the session, while a late detection
   costs one command's boundaries and then recovers, because
   `SessionState::markers_observed` already upgrades from `Unintegrated` (DESIGN decision
   8's recovery). This is the number in this spec most likely to be retuned by NVDA
   evidence, and it is named as such.

10. **An unintegrated session degrades honestly: submission becomes the boundary.**

    DESIGN's reliability case 2 is **Decided** — flag the session, announce it, and
    degrade every command to case 1: patience announcement, manual buffer review, no
    auto-read. None of it happens today. B3.5's `pipeline.rs` asserts that an unmarked
    session produces **zero events**, and its message calls that correct: "with no block,
    there is no command to report". That was right for glue and is not shippable as the
    default backend — it is a silent terminal with an empty buffer.

    The obstacle is structural, not a filter to relax: every `SessionEvent` carries a
    `command_id` and the actor's whole model is `ActiveCommand`, so with no `BlockStarted`
    there is no slot for the text to occupy.

    So when integration resolves to `Unintegrated`, the service opens a command at
    submission time — it already minted that id for the `SubmitAck` — and closes it when
    the next line is submitted. Text reaches the buffer, patience still fires, auto-read
    is suppressed. Three consequences, written down rather than discovered:

    - **Echo exclusion is lost.** With no regions there is no C..D to cut, so the shell's
      echo of the submitted line appears in the output. Unavoidable without markers; it is
      a stated cost of honest degradation, not a defect.
    - **No exit codes.** A command that never ends structurally has none to report. On the
      wire that is `ExitCode(0)` — the silent value, the same one decision 8 gives a bare
      `D` — because `CommandFinished` carries a code and the frontend announces only
      nonzero ones. Nothing claims the command succeeded; nothing is said at all.
    - **Late recovery is free**, per decision 9.

    **Amended during implementation, in three places.** Each is a case the rule above does
    not reach, and each was answered the way this decision answers everything else: the
    honest degradation, not the silent one.

    1. **An interrupt is a boundary too.** With no markers there is no `D` coming, ever,
       so a command stopped with Ctrl+C would stay open until the next submission and the
       stop would be announced minutes later, or never. In a session with no boundaries
       the interrupt *is* the boundary: the service closes the open command as soon as it
       has asked the transport to stop it. Waiting for a confirmation that cannot arrive
       is the silent-terminal failure this decision exists to prevent, and the service is
       reporting what it did rather than what the far end did — which is what decision 8
       already established `CommandInterrupted` means.

    2. **A line submitted while integration is still `Pending`.** The rule opens a command
       at submission only once integration has *resolved* to `Unintegrated`, so a line
       typed inside the grace period has no command to occupy and its output is dropped
       until the period expires. That cost is real and it is bounded: it is at most the
       first command of a session that is already degrading, and it is the mirror image of
       the cost decision 9 already accepts in the other direction ("a late detection costs
       one command's boundaries and then recovers"). It is also rare by construction —
       markers arrive with the shell's *first prompt*, before any submission, so a session
       that is still `Pending` when the user types is one that is going to be flagged
       anyway. What the service does add is the cheap half: when the grace period expires
       with a submitted id no block ever claimed, that command opens then, so everything
       arriving from that instant on has somewhere to go. Only the most recent such id,
       since the shell is serial.

    3. **A recovering session adopts its degraded command rather than orphaning it.** When
       a late marker recovers the session (decision 9) while a degraded command is open,
       the `BlockStarted` that follows is that same command finally announcing itself. It
       keeps the id the ack already gave the frontend instead of minting a fresh one, so
       the buffer block the user is looking at — the one headed with the line they typed —
       goes on to receive the real output and the real exit code. Closing it and opening a
       second one beside it would leave the labelled block empty and the full one
       unlabelled, which is a worse answer than the ambiguity it avoids. The tracker never
       nests blocks, so an already-open command at `BlockStarted` can only ever be this
       case.

11. **The unintegrated announcement is a `SessionEvent`, not an `Announcement`.**

    `SessionEvent::Announce` carries a `command_id`, and this announcement fires when the
    grace period expires — at session start, before any command exists. So it takes the
    shape `AltScreenEntered` and `AltScreenLeft` already have: a session-scoped event
    carrying no command id, which the frontend maps to a pinned string it owns.

    `SessionEvent::IntegrationUnavailable`, and a pinned string in
    `ui/src/controllers/app.ts` beside the others. Reusing `ConnectionState` was rejected:
    it describes the transport, and conflating "the pipe is down" with "the shell did not
    announce itself" would make two different failures sound alike to a listener.

12. **`FakeSessionService` is deleted, not maintained beside the real service.**

    Agreed in conversation 2026-08-18 and recorded in B3.5 decision 2: after B3.5, faking
    is a *transport* choice, so from here there is exactly one session service. DESIGN's
    Decided "the scripted fake session is a permanent supported session kind" survives
    unchanged — the scenarios became data in B3.5; what is deleted is the imitation of a
    domain that now runs for real.

    Deleted with it: `acter-app`'s `entities/fake_script.rs` and the `ACTER_FAKE_SCRIPT`
    environment variable. Replaced by `ACTER_TRANSCRIPT`, which accepts **either a
    built-in name or a path** to a transcript JSON, falling back to
    `SessionTranscript::builtin()` when unset. Same container privilege and the same
    loud-failure rule: an unknown name, or a read or parse failure, is a startup panic
    naming what was asked for, never a silent fallback.

    The name form is worth the few lines it costs. DESIGN Decided that a profile bundles a
    transport, a shell adapter and settings, and that the scripted fake session is a
    permanent session kind "selectable like any real shell once the session/profile UI
    exists". That UI is post-convergence, but the *content* a simulated profile would
    select is already fixture data — B3.5's reliability transcripts are, in everything but
    name, simulated shells: one that never emits markers, one that forges them, one that
    splits a marker across two reads. Naming them makes them selectable now, by a human
    doing a manual NVDA run, without building any configuration machinery. The profile
    that eventually selects one is a lookup that already resolves.

    **Updated after B3.6: a name resolves to a composition, not only to a file.** Two of
    those three stopped being transcripts. "Never emits markers" is
    `Unmarked::new(TranscriptShell::builtin())`, a decorator over any far end; "splits a
    marker across two reads" is `Chunking::Bytes(1)` over *any* transcript, since read
    boundaries belong to the pipe. Only the forged marker is still a transcript, and
    correctly so — forging one is something a *program* does, which is a property of the
    command that was run (B3.6, decision 5). So the lookup this passage argues for resolves
    a name to a shell plus a chunking, and the set of nameable simulated profiles is now
    the product of the two rather than a list of files somebody wrote. That is a larger
    payoff than this decision originally claimed, at the same few lines of cost.

    **As implemented**, that product is four names — `builtin`, `builtin-by-byte`,
    `unmarked`, `unmarked-by-byte` — and anything else is taken as a path to a transcript,
    read whole. A forged marker stays a transcript and so stays a path, correctly: forging
    one is something a program does. A name that is neither in the table nor loadable as a
    file is a startup panic naming what was asked for and listing the table, because a
    manual accessibility run that quietly tested the wrong session would be worse than one
    that did not start.

## Deliverables

### `acter-core`

- `services.rs` + `services/session.rs` — `SessionService`, the component above.
- `ports/driven/transport.rs` — `Transport::interrupt` (decision 5).
- `ports/driving/session_api.rs` — `SessionApi::send_key` (decision 4).
- `entities/protocol_commands.rs` — `KeyPress`, `Key`, `KeyAck` (decisions 4, 6, 7).
- `entities/session_intent.rs` — `SessionIntent`, the domain-side meaning (decision 6).
- `policies/keybindings.rs` — the `KeyPress` to `Option<SessionIntent>` table (decision 6).
- `entities/protocol_events.rs` — `SessionEvent::IntegrationUnavailable` (decision 11).
- `entities/pacing_state.rs` — `PacingConfig::integration_grace` (decision 9).
- `controllers/session_actor.rs` — `SessionInput::{MarkersObserved, GracePeriodExpired}`,
  the integration transitions applied to the `SessionState` it already owns, and auto-read
  suppression while `Unintegrated` (decisions 9, 10).

### `acter-transports`

- `ScriptedTransport::interrupt`, routed into the existing interrupting-rule path.

### `acter-app`

- `container.rs` — builds `SessionService` over `ScriptedTransport` and
  `AlacrittyEngine`; resolves `ACTER_TRANSCRIPT` as a built-in name or a path;
  `acter-term` and `acter-transports` become dependencies.
- `routers/session.rs` — the `send_key` invoke.
- **Deleted:** `services/fake_session.rs`, `services.rs`, `entities/fake_script.rs`, and
  `entities.rs` if nothing remains in it.

### `ui`

- Regenerated protocol bindings (`cargo test -p acter-app --test protocol_bindings`).
- `controllers/app.ts` — a pinned string for `IntegrationUnavailable` and its handler
  arm; A2's exhaustiveness guard forces this rather than leaving it optional.

### Docs

- `ROADMAP.md` — B6 flipped to Done; A3.2 unblocked and rescoped to a frontend keystroke
  handler over `send_key`; EOF filed as a B5 item; read timing filed against B4 (see Out
  of scope), with whatever the accessibility matrix found recorded against it.
- `ARCHITECTURE.md` — the reference layout's `acter-app` bullets, which still describe the
  A3 fake as the wired backend.
- `DESIGN.md` — the injection-retry idea recorded as an open question (see Out of scope).

## Tests

- **The B3.5 pipeline test survives with its glue replaced by the real service.** That is
  the promotion, and it is the strongest regression net available: the same transcripts,
  the same assertions, a real component in place of fifty lines of test scaffolding.
- Unit tests against fake driven ports for what the glue never had: correlation including
  both edges of decision 3; interrupt producing `CommandInterrupted` rather than
  `CommandFinished`, and a bare `D` with no outstanding interrupt still finishing;
  `KeyAck` distinguishing an unbound key from a bound key with nothing running; the
  keybinding policy as a table test, including that an unbound key maps to `None`.
- The grace period, driven by the fake clock: `Pending` to `Unintegrated` with no markers,
  `Pending` to `Integrated` on a marker, and recovery from `Unintegrated` on a late one.
- `pipeline.rs`'s unmarked-session test **rewritten**: it currently asserts silence and
  must assert honest degradation — the announcement, output reaching the buffer, patience
  firing, and no auto-read. Since B3.6 it runs over
  `Unmarked::new(TranscriptShell::builtin())` rather than a fixture, so the rewrite is to
  its assertions only; the far end it degrades is the full built-in shell, which is what
  makes "every command degrades to case 1" assertable over more than one scripted line.
- Nothing sleeps or reads the real clock, per B3.5 acceptance criterion 2.

**What the promotion did to `pipeline.rs`, beyond replacing the glue.** Four things, none
of them a change of subject:

- The suite runs with `integration_grace` at 200ms rather than the shipped five seconds,
  so a session that is going to be flagged is flagged inside the scripted time each case
  already runs for. That the default is five seconds is `PacingConfig`'s own test to pin.
- `Substance`'s `integrated` flag was the glue's own boolean and is now `unintegrated`,
  read off `SessionEvent::IntegrationUnavailable` — the same claim, made from an
  observable rather than from test scaffolding.
- The unmarked case in the replay suite gains a warm-up before its first submission, so it
  exercises the degraded path rather than the amendment above; the suite's exception branch
  now asserts one degraded block with output and no exit code, instead of no blocks at all.
- `a_resize_reaches_the_transport` is deleted rather than rewritten. It asserted
  `ScriptedTransport::last_resize`, which that crate's own unit tests already pin, and the
  service exposes no resize path for it to go through — `SessionApi` has no resize until
  something asks for one.

## Acceptance criteria

1. `cargo test --workspace` green; `cargo clippy --workspace --all-targets` clean under
   `-D warnings`; `cargo fmt --check` clean; `npm test` green.
2. The default backend is the byte path: launching the app with no environment set runs
   the built-in transcript through the real engine, tracker, policy and actor.
3. `FakeSessionService` and `FakeScript` are gone from the tree, and no test reintroduces
   an event-level fake.
4. Module role declared on the first line of each new module's `//!` comment; the
   visibility ladder holds; `acter-core` still names no adapter crate.
5. Protocol bindings regenerated and the drift guard green; the frontend's exhaustiveness
   guard compiles with the new event handled.
6. `Key` and `SessionIntent` each have exactly one variant, the keybinding policy is a
   pure function with no I/O, and `Transport::interrupt` has a working implementer
   in this PR.
7. **Accessibility checklist in the PR body**, one item per check. B3.5 decision 9 makes
   this the entry where the manual matrix tests the real policy for the first time, so the
   ten scenarios are re-run through the byte path, plus the unintegrated session and an
   interrupt. Items the agent can observe through the screen-readers bridge are recorded
   with the NVDA version and capture mode; items turning on a beep or subjective comfort
   stay human-verified, and the PR body states which is which.

## Out of scope

- **The Ctrl+C keystroke handler and its selection check** — A3.2, which this PR unblocks
  and reduces to frontend work over a method that now exists with a real backend behind
  it. Note that A3's alt-screen string already tells the user to press Ctrl+C, so the
  promise stays unkept until that entry lands.
- **EOF** — B5's, for the reason in decision 6.
- **Splitting `ScriptedTransport` into a fake shell and a fake pipe.** Raised in
  conversation 2026-08-19, and a real defect in the current factoring rather than a
  preference. Two things are fused in it: what the far end *produces* (prompt, echo,
  markers, output, timing) and how those bytes *arrive* (chunking, a marker split across
  two reads, the far end going away). B3.5 encodes the second as fixture data, so
  "a marker split across two reads" is a transcript when splitting is a property of the
  pipe. The cost is composition: exercising every fixture under adversarial chunking means
  hand-authoring the split into each one instead of pairing any transcript with any
  delivery strategy.

  The right shape is a `FakeShell` that answers input with bytes and a delivery strategy
  that decides how they reach the channel, with `ScriptedTransport` composing the two.
  Note what it is **not**: a `ShellAdapter`. That port is knowledge the domain *calls for*
  — injection snippet, quoting rules, completion strategy — and it sits outside the byte
  path entirely. The shell in the byte path is the far-end process, and there is no port
  for it because the transport *is* the seam to it. Which is why the composition only runs
  one way: a real shell adapter over a fake transport is realizable and useful, while a
  fake shell behind a real `LocalPty` is not, since ConPTY spawns an actual process. So
  this is an internal decomposition of one adapter crate, not a new port, and the profile
  story is unchanged — a scripted profile still fills the transport slot with
  `ScriptedTransport` and the shell-adapter slot with a null adapter.

  **Done, and not at B5: it landed as B3.6, before this entry.** This paragraph used to
  defer it to B5 on the grounds that this PR adds only `interrupt` and so does not deepen
  the conflation. That argument was right about the cost and wrong about the order. B6
  switches the default backend and runs the first real accessibility matrix against these
  fixtures, so splitting afterwards would author them twice — and B3.6 cost this entry
  nothing, since `interrupt` is one line either way (see decision 5's update). Spec:
  [b3.6-fake-shell.md](b3.6-fake-shell.md).

  What B6 inherits from it: `ScriptedTransport` is now a `Box<dyn FakeShell>` plus a
  `Chunking` plus a `Clock`; the unintegrated far end decision 10 degrades is
  `Unmarked::new(TranscriptShell::builtin())` rather than a fixture; and every fixture is
  already replayed byte-at-a-time with its output text, blocks, exit codes, marker
  recognition and announcements asserted unchanged. The shape this paragraph predicted is
  the shape that shipped, including that it is not a `ShellAdapter`.

- **Read timing: a pipe that spaces its reads apart in time.** Raised in conversation
  2026-08-19, after B3.6, and worth having.

  What is already simulated is the far end's timing: every delivery carries a delay taken
  from the `Clock` port, which is what makes `slow`, `tail`, `burst` and `forever` mean
  anything. What is not simulated is the gap *between the reads of one delivery*.
  `ScriptedTransport::send` cuts a delivery with `Chunking` and pushes every read into the
  channel with no clock advance, so under `Bytes(1)` all two hundred and fifty bytes of a
  flood land at the same instant. The suite therefore proves that cutting never loses text
  and never breaks a marker, and says nothing about a line whose halves arrive on either
  side of the half-second quiescence window the pacing policy uses to decide a line is
  done.

  **It buys less than it first appears to, and that is the reason to be precise about
  it.** The domain sees only bytes and arrival times, so a shell that pauses mid-line and a
  pipe that delays half a read are indistinguishable to it — and the first is already
  authorable as two steps with a delay between them. What a timing pipe adds is not
  expressiveness but *uniformity*: exactly what `Chunking` added, turning something
  expressible-but-expressed-nowhere into a dimension every fixture runs under. Today no
  fixture delivers a partial line across a quiescent gap, so what the real policy does with
  one is untested.

  **It is not free, which is why it is its own entry rather than a line here.** `Chunking`
  is currently a pure policy — deterministic in, deterministic out, unit-tested with no
  runtime — and spacing reads apart either puts the clock inside it or splits the concept
  in two. Worse, the natural implementation is `send` calling the pipe's existing `wait`
  between reads, which would let an interrupt cancel a command *mid-delivery*, halfway
  through a marker. That is arguably more realistic and definitely a behavior change, and
  it deserves a decision made in the open rather than inherited from an implementation
  convenience.

  **B4 is the natural moment.** That is when `LocalPty` on ConPTY makes real read timing
  observable, so the fake's model can be built against a pattern somebody measured instead
  of one somebody guessed — the same discipline this spec already applies to designing
  `ShellAdapter` against a fake alone. One thing would promote it earlier: if this entry's
  accessibility matrix turns up a line announced before it was finished, that is the
  evidence, and it becomes the next entry rather than a later one.

- **A null `ShellAdapter` and profiles as a configuration surface.** Also B5's, for a
  different reason: that is when the trait first ships with PowerShell behind it, and when
  the scripted session needs something in a profile's shell-adapter slot to be "selectable
  like any real shell" as DESIGN Decided. Introducing the trait here with only a fake
  implementer is the shape B1 refused to create — the rule this spec already applies to
  `Eof` and to `Key`'s named variants, and it cannot be waived for this one.
- **`LocalPty` on ConPTY** — B4. `Transport::interrupt` is part of the seam it implements.
- **Retrying a second injection when the first produces no markers.** Raised in
  conversation 2026-08-19 and worth having: an unintegrated session could try an
  alternative injection before giving up, which would make case 2 rarer rather than merely
  honest. It needs `ShellAdapter` to exist and belongs to a design conversation about what
  a session's startup handshake is, so it is recorded as a DESIGN open question here and
  decided no earlier than B5.
- **`Output.read_mode`, `CommandStillRunning` and failure-via-`CommandFinished`** — A6's,
  unchanged from B3.5.
- **Multiple concurrent sessions and tabs** — `session_manager.rs` is post-convergence;
  this service owns one session, and `SessionId` is carried on every call as it already
  is.
