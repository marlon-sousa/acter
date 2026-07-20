# Spec: PR A3 — fake session backend

Replaces the A1 echo harness with a scripted fake `SessionApi` backend speaking the
A2 protocol over the real Tauri Channel path. This is the first producer of
`SessionEvent`s and the PR that unlocks the full manual NVDA matrix: auto-read,
too-big + beep, exit-code announcements, pacing announcements, alt-screen notice.
No real shell, no policy computation — every verdict and delay is scripted, so
nothing here needs un-writing when B1's pacing policy and B6's real service land.

## Why now / relation to the roadmap

- Roadmap lane 1, entry A3. Scope sketch: "scripted SessionApi fake (small output,
  over-threshold output, failing command, slow command, never-ending command) wired
  into the harness; unlocks the full manual matrix."
- The roadmap flagged two decisions for this spec: where the beep lives and how the
  frontend renders pacing verdicts. Both are resolved below (decisions 1 and 2),
  agreed in conversation on 2026-07-18. The fake script config and the
  fake-as-default-backend decisions (8 and 9) were agreed in the follow-up
  conversation on 2026-07-19.
- UI-first principle: manual NVDA testing is the slowest feedback loop; this PR
  starts it for the whole announcement surface using scripted events, months before
  a real shell exists. Because fake and real service implement the same `SessionApi`
  trait and protocol, validated UI behavior carries over unchanged at convergence.

## Design decisions this spec makes

1. **The beep is a frontend view adapter, not a backend port.** The beep renders the
   `ReadMode` verdict exactly as the live-region announcement does; all audible
   output lives in one layer (`ui/src/adapters/beep.ts`, WebAudio, behind
   `ports/beep_view.ts`). This resolves a latent contradiction in ARCHITECTURE.md,
   which listed `Notifier` (beep/audio) as an upcoming backend driven port while
   also listing a frontend "beep player" view adapter and describing the too-big
   beep as UI behavior. Resolution, recorded in ARCHITECTURE.md in this PR:
   `Notifier` is removed from the known-upcoming-ports list; if a genuinely
   backend-side notification need appears later (e.g. OS-level toasts for
   background-tab activity), it gets its own port then. A proposed, non-silent
   change.
2. **Verdict rendering rules.** The buffer always receives output text; verdicts
   govern speech only:
   - `Output` with `Auto`: append text under the command's block and announce the
     text via the live region.
   - `Output` with `TooBig`: append text, announce "N lines arrived, too big to
     read". N is the line count of the received chunk, computed by the frontend —
     this is phrasing, not re-measuring: the verdict was already made backend-side.
     No protocol change.
   - `Output` with `Quiet`: append text, no speech.
   - `CommandFinished` with nonzero exit code: announce the failure string (always
     spoken, regardless of verdicts).
   - Beep trigger: the frontend remembers per command whether any of its chunks
     carried `TooBig`; on that command's `CommandFinished`, beep (also when the
     finish verdict itself is `TooBig`). This matches the user-facing promise —
     "you were told it is too big; the beep tells you it is done" — and is view
     state the frontend legitimately owns. A successful, fully auto-read command
     gets no extra finish speech: its output was already read.
   - `CommandStillRunning`: announce the patience string.
   - `AltScreenEntered` / `AltScreenLeft`: announce the alt-screen strings.
   - `TitleChanged` / `ConnectionChanged`: handled in the exhaustive switch as
     no-ops for now (no UX decided; producers arrive later anyway).
3. **Pinned announcement strings.** Every announced string is a domain requirement
   (CLAUDE.md). The exact strings, with N and code as decimal numbers:
   - Too-big chunk: `N lines arrived, too big to read`
   - Failure: `command failed, exit code N`
   - Patience: `long command running, output is accumulating in the buffer`
   - Alt-screen entered: `this program needs interactive mode, which is not
     available yet. Press Ctrl+C to return to the prompt`
   - Alt-screen left: `interactive program ended`
   (Ctrl+C interrupt wiring is out of scope; the fake's alt-screen scenario exits
   on its own. The string is pinned now because it is the phase-1 decided response
   to alt-screen and should be NVDA-validated as worded.)
4. **Scenario selection by command name.** The typed line picks the script (table
   below); anything unrecognized echoes, preserving the A1 manual-testing loop.
   Manual sessions become self-describing and E2E gets deterministic hooks.
5. **The fake scripts, never computes.** Verdicts, delays, and event order are baked
   into each scenario shape. The quiescence/patience policy (B1) and boundary
   tracking (B2) are not implemented, approximated, or partially anticipated. All
   numbers come from the fake script config (decision 8); unit tests run an
   all-zero-delays config.
6. **`get_session_snapshot` is re-deferred.** A2 deferred it to A3; A3 defers it
   again — the fake has no session state worth snapshotting, and webview-reload
   recovery is not yet testable in any meaningful way. It lands with the real
   session model (convergence, or the first PR that can honestly test recovery).
7. **Fake command semantics deviation (accepted).** DESIGN.md decides that a line
   submitted while a command runs is stdin to that program. The fake has no stdin;
   in A3 every Enter opens a new command block, and concurrent scripted commands
   may interleave events (correctly demultiplexed by `command_id`). The stdin rule
   arrives with the real domain; this deviation is acceptable because A3 validates
   announcement UX, not shell semantics.
8. **The fake is configuration-driven: the fake script config.** Every number in
   the scenario scripts — delays, chunk sizes, line counts, iteration counts, exit
   codes — comes from a config value, not a constant. Delays are ranges
   (`min_ms`/`max_ms`): equal bounds give E2E determinism, unequal bounds give
   manual sessions organic pacing. Built-in defaults in code carry the human-scale
   manual-testing values, so `tauri dev` needs no file; an optional JSON file
   overrides them, its path read by the container from the `ACTER_FAKE_SCRIPT`
   environment variable. The E2E harness writes a temp config with small
   deterministic numbers and sets the variable when spawning the app. Two scope
   lines, agreed in conversation: this is **not** the Profile domain (DESIGN's
   profiles — transport + adapter + settings with inheritance — are
   post-convergence; this file configures only the fake and is named accordingly),
   and scenario *shapes* stay in code — parameterized, not a step-scripting DSL
   (additive later if experimentation demands it). The purpose is double: E2E and
   manual testing share one fake at different time scales, and the pacing numbers
   the user tunes by feel become evidence for B1's real policy defaults.
9. **The fake is a permanent session kind and the Phase-1 default backend.** The
   frontend attaches to the fake automatically at startup (`SessionId` 1) — a
   connection is established on load with no user action. When the session/profile
   UI lands (post-convergence), the fake appears in the supported-sessions
   dropdown as a first-class kind alongside real shells: convergence swaps the
   *default*, it does not delete the fake. A scriptable session stays permanently
   useful for accessibility experiments, demos, and reproducing NVDA findings
   without a real shell. Recorded as a product decision in DESIGN.md (profiles
   section) in this PR.

## Scenario scripts

All numbers below are the **built-in defaults of the fake script config**
(decision 8) — the human-scale manual-testing values. E2E and unit tests override
them (unit tests run an all-zero-delays config). Each script runs on its own
backend task, emitting through the session's event sink.

- `small` — `CommandStarted`; after 100 ms `Output` ("hello from acter", `Auto`);
  `CommandFinished` (exit 0, `Auto`). The plain auto-read case.
- `big` — `CommandStarted`; after 100 ms `Output` (40 numbered lines, `TooBig`);
  after 1.5 s `CommandFinished` (exit 0, `Auto`). Exercises the too-big
  announcement and the completion beep.
- `fail` — `CommandStarted`; after 100 ms `Output` (one short error line, `Auto`);
  `CommandFinished` (exit 2, `Auto`). Exercises the failure announcement.
- `slow` — `CommandStarted`; three `Auto` chunks ("phase one/two/three") at 1 s
  intervals; `CommandFinished` (exit 0, `Auto`). Phase-by-phase narration.
- `forever` — `CommandStarted`; two `Auto` chunks 1 s apart; after 2 s more,
  `CommandStillRunning`; then a `Quiet` chunk every 2 s indefinitely. No
  `CommandFinished` ever. Exercises patience announcement, silent accumulation,
  and UI responsiveness under an endless stream.
- `nano` — `CommandStarted`; after 300 ms `AltScreenEntered`; after 5 s
  `AltScreenLeft`; `CommandFinished` (exit 0, `Quiet`). Exercises the phase-1
  alt-screen announcements. (Addition beyond the roadmap sketch, agreed in the
  spec conversation: the announcement UX is cheap to script and worth NVDA
  validation now; the real producer arrives with B3.)
- `tail` — `CommandStarted`; then N iterations (default 10) of one small `Auto`
  chunk ("tail line K") with a randomized delay in a range between iterations
  (default 3 to 8 s); then `CommandFinished` (exit 0, `Auto`). The
  buffer-reading experiment: lets the user read earlier buffer content in browse
  mode *while* new chunks arrive and are announced — browse-mode stability and
  interruption behavior under live output is a finding only manual NVDA testing
  can produce. (Addition agreed in the spec conversation, 2026-07-19.)
- `burst` — `CommandStarted`; after a short delay one huge `TooBig` chunk
  (default 60 numbered lines — the too-big announcement is heard); then M
  iterations (default 4) of a small `Auto` chunk on the same delay-range pacing
  as `tail`; then `CommandFinished` (exit 0, `Auto`) — the beep fires (the
  command had a too-big chunk). The sudden-flood-then-trickle case: verifies
  small following chunks are still read aloud after a too-big announcement, and
  that the completion beep survives intervening auto-read chunks. (Addition
  agreed in the spec conversation, 2026-07-19.)
- `speech` — `CommandStarted`; after 100 ms one long `Auto` phrase (an opening
  marker, `word_count` numbered words — default 60, roughly twenty seconds of
  speech — and the closing marker `long announcement finished`);
  `CommandFinished` (exit 0, `Auto`). Exercises the live-region clear (amendment
  below): the phrase is deliberately longer than the clear delay, so a manual NVDA
  pass hears whether emptying the region truncates queued speech. If the closing
  marker is spoken, nothing was lost; if speech stops, it stops on a numbered word
  that names exactly how far it got. (Addition agreed in conversation 2026-07-20,
  with the live-region-clear amendment.)
- anything else — `CommandStarted`; `Output` (the line itself, `Auto`);
  `CommandFinished` (exit 0, `Auto`). The echo fallback.

## Deliverables

### `acter-core` — the first ports

- `ports.rs` + `ports/driving/session_api.rs`: trait `SessionApi` (role: port).
  Synchronous methods per ARCHITECTURE: `attach_session(&self, session: SessionId,
  sink: Arc<dyn EventSink>)` and `submit_command(&self, session: SessionId,
  line: &str) -> SubmitAck`. Phase 1 has one session; the id is carried anyway
  (Decided: commands carry `session_id` as an argument).
- `ports/driven/event_sink.rs`: trait `EventSink: Send + Sync` with
  `send(&self, event: SessionEvent)` (role: port). The seam the whole event
  pipeline is tested through.
- `lib.rs` facade re-exports both traits.
- Ports are trait declarations — no behavior, no tests (same exemption as facades).

### `acter-app` — fake service, script config, routers, channel adapter

- `Cargo.toml`: `acter-core` becomes a normal dependency (was dev-only since A2);
  `serde` and `serde_json` become normal dependencies (config parsing).
- `entities/fake_script.rs` (new `entities.rs` facade): the fake script config
  (role: entity/value) — serde structs for the per-scenario parameters, the
  `DelayRange { min_ms, max_ms }` value type, and the built-in `Default`
  implementation carrying the manual-testing numbers from the scenario table.
  Pure data + parsing; unit tests cover parsing, defaults, and rejection of
  invalid ranges (min greater than max).
- `services/fake_session.rs`: `FakeSessionService` implements `SessionApi` (role:
  service). Constructor takes the fake script config as data. Holds the attached
  sink; `submit_command` allocates the next `CommandId`, spawns a thread that
  plays the scenario script with the configured delays (randomized within range),
  and returns `SubmitAck` immediately (invoke never waits on the shell).
- `adapters/channel_sink.rs` (new `adapters.rs` facade): `ChannelSink` implements
  `EventSink` over `tauri::ipc::Channel<SessionEvent>` (role: adapter) — the only
  place a Channel is held, per ARCHITECTURE.
- `routers/session.rs`: `attach_session` (receives the JS `Channel<SessionEvent>`,
  wraps it in `ChannelSink`, passes it to `SessionApi`) and `submit_command`
  (primitive args: `session_id`, `line`; returns `SubmitAck`). One-liners, facade
  glob re-export for the `__cmd__` companions.
- `container.rs`: reads `ACTER_FAKE_SCRIPT` — if set, loads and parses the JSON
  file (a parse failure is a loud startup error, never a silent fallback);
  otherwise uses the built-in defaults. Manages `Arc<dyn SessionApi>` =
  `FakeSessionService` with that config; registers the new routers. Reading env
  and file here is fine: the container is the composition root, where the world
  is allowed in.
- **Echo harness removed:** `ports/echo_api.rs`, `services/echo.rs`,
  `routers/echo.rs` deleted; A1's temporary domain is done.

### Frontend

- `ports/backend_api.ts`: `echo` replaced by
  `attachSession(onEvent: (event: SessionEvent) => void): Promise<void>` and
  `submitCommand(line: string): Promise<SubmitAck>`.
- `ports/beep_view.ts` + `adapters/beep.ts`: `BeepView.beep()`; WebAudio short tone
  (880 Hz, 150 ms) — the decided home of the beep.
- `ports/buffer_view.ts` + `adapters/buffer.ts`: blocks keyed by `CommandId` —
  `openBlock(commandId, commandLine)` (h2 = the command line, as in A1) and
  `appendOutput(commandId, text)`. F6/heading behavior from A5.1 is preserved.
- `routers/tauri.ts`: typed invoke wrappers plus the JS `Channel<SessionEvent>`
  creation for `attachSession` — still the sole importer of `@tauri-apps/api`.
- `controllers/app.ts`: on submit, calls `submitCommand`, opens the block on the
  ack (per the ARCHITECTURE round-trip: block appears immediately, tagged with the
  id); handles `SessionEvent` with the exhaustive `assertNever` switch from A2's
  guard, applying decision 2's rendering rules; tracks the per-command too-big
  flag for the beep. Defensive rule: an event whose `command_id` has no open block
  lazily opens one (covered by a unit test), so a scripting race cannot lose
  output.
- `main.ts`: wires the beep adapter; attaches the session at startup with
  `SessionId` 1 — the fake is the default backend, connected automatically on
  load with no user action (decision 9).

### Tests

- `acter-app` unit: fake script config — parsing a full JSON file, defaults when
  absent, loud rejection of invalid ranges. `FakeSessionService` with a recording
  `EventSink` fake and an all-zero-delays config — one table test asserting each
  scenario's exact event sequence (including `tail` and `burst` with small
  configured iteration counts), plus the ack-precedes-events and
  unknown-command-echo cases.
- `acter-app` router integration (T1 pattern, mock runtime): `submit_command`
  returns a `SubmitAck` through the real invoke pipeline. Channel delivery through
  the mock runtime is attempted; if the mock runtime cannot carry a Channel, the
  E2E suite is the named owner of that path (finding recorded in the spec
  amendment if so).
- `ui` vitest: controller tests with fake backend, views, and beep — every
  rendering rule in decision 2, including line-count phrasing, the too-big beep on
  finish, failure always spoken, quiet appends, lazy block opening, and the
  no-op variants.
- E2E (WebdriverIO): the harness writes a temp fake script config (all delay
  ranges small with min equal to max — deterministic — and small iteration
  counts) and sets `ACTER_FAKE_SCRIPT` when spawning the app. Updated specs off
  the echo surface — smoke types `small` and asserts the block + output text;
  announcer asserts the auto-read announcement (announce-once semantics
  preserved); `big` produces the pinned too-big announcement text; `tail` with
  two fast iterations asserts chunks append to the same block in order. The beep
  itself is manual-checklist territory (WebDriver cannot hear).
- All A2 guards (bindings drift step, protocol tests) run unchanged — no protocol
  edits in this PR.

### Docs

- `ARCHITECTURE.md`: remove `Notifier` from the known-upcoming-ports list and note
  the beep is a frontend view adapter (decision 1); add the new files to the
  reference layout (`acter-core` `ports/driving/session_api.rs`,
  `ports/driven/event_sink.rs`; `acter-app` `entities/fake_script.rs`,
  `services/fake_session.rs`, `adapters/channel_sink.rs`, `routers/session.rs`;
  ui `ports/beep_view.ts`, `adapters/beep.ts`).
- `DESIGN.md` (profiles section, one sentence, **Decided**): the scripted fake
  session is a permanent supported session kind — selectable like any real shell
  once the session/profile UI exists, and the default backend until then
  (decision 9). Approved in conversation 2026-07-19; rides in this PR.
- `ROADMAP.md`: this PR flips lane-1 entry A3 to Done and sets its Spec field to
  this file.

## Acceptance criteria

1. Typing each scenario name in the running app produces exactly its scripted
   event sequence, rendered per decision 2; unknown lines echo.
2. `cargo test --workspace` green: fake-service table tests (zero delay), router
   integration tests, plus all existing suites.
3. `npm run typecheck`, `npm run test:ui` green: controller rendering rules fully
   covered with fakes; the `SessionEvent` switch remains exhaustive (A2 guard).
4. E2E suite green with the updated specs (smoke via `small`, announcer, too-big
   text via `big`, axe unchanged).
5. The echo harness is gone: no `EchoApi`, no `echo` router, no `echo` method on
   `BackendApi`.
6. Every pinned string in decision 3 appears verbatim in exactly one place in the
   frontend (single source for each announcement).
7. The fake is config-driven end to end: the app runs with built-in defaults when
   `ACTER_FAKE_SCRIPT` is unset; pointing it at a JSON file changes scenario
   numbers without recompiling; a malformed file fails startup loudly; E2E runs
   entirely on its own generated config.
8. On launch, the app is attached to the fake session with no user action —
   typing a scenario name immediately works (decision 9).
9. Manual NVDA checklist (below) completed in the PR body; findings spawn A5.x
   entries.

## Out of scope

- Real shells, transports, terminal engine, OSC 133 — lanes B1 to B6.
- Pacing policy computation (quiescence, patience windows, babble guard) — B1. The
  fake's delays and verdicts are scripted stand-ins.
- stdin to a running command / mid-command prompts — real domain (decision 7).
- Command history, completion (A4), `get_session_snapshot` (decision 6),
  Ctrl+C interrupt, follow mode, Ctrl+Shift keybindings beyond those already
  shipped, line-cap rendering for endless output (the `forever` scenario relies on
  the buffer simply growing; a cap gets its own entry when it becomes a measured
  problem).
- `TitleChanged` / `ConnectionChanged` UX — no producers, no rendering decisions.
- The Profile domain and the session/profile selection UI (supported-sessions
  dropdown, session manager, tabs) — post-convergence. Decision 9 only records
  that the fake will be listed there; nothing is built for it now.
- A step-scripting DSL for the fake (decision 8's second scope line) — additive
  later if parameterized shapes prove insufficient.

## Manual accessibility checklist (PR body)

One checkbox per item; findings written inline on the unchecked item (NVDA
version, expected vs observed):

- `small`: output is read automatically once, no double-speak from buffer focus.
- `big`: the too-big announcement is spoken with the correct line count; the
  output text is NOT read aloud; the beep sounds when the command finishes.
- `fail`: the failure announcement is spoken with the exit code; output text
  still readable in the buffer.
- `slow`: each phase chunk is read as it arrives, in order, no chunks lost or
  merged by the screen reader.
- `forever`: the patience announcement is spoken once; subsequent quiet chunks
  are silent; the app stays responsive; the buffer accumulates; typing and
  submitting another command while it runs still works.
- `nano`: alt-screen entered/left announcements are spoken as pinned.
- `tail`: while chunks keep arriving, reading earlier buffer content in browse
  mode works — new-chunk announcements do or do not interrupt reading (observe
  and record which), the virtual cursor position is not yanked, and focus stays
  where the user put it.
- `burst`: the too-big announcement is heard for the flood; the following small
  chunks are read aloud normally; the completion beep still fires at the end.
- Fake script config: with a hand-edited JSON file and `ACTER_FAKE_SCRIPT` set,
  changed timings are audibly in effect (spot-check one scenario).
- Echo fallback: an arbitrary line is echoed and read (A1 behavior preserved).
- F6 / heading navigation: h2 per command still present; F6 focuses the most
  recent command heading (A5.1 behavior intact with the new block structure).
- Beep volume/pitch is comfortable alongside NVDA speech (subjective check).

## Amendment (during implementation)

- **Router integration test scope: E2E owns Channel delivery.** The mock-runtime
  router test (`crates/acter-app/src/routers.rs`) exercises `submit_command` returning
  a `SubmitAck` through the real invoke pipeline, plus the missing-argument error path.
  Delivering a live `tauri::ipc::Channel` through `tauri::test`'s `get_ipc_response`
  is not supported by the mock runtime — a Channel's downstream delivery is bound to a
  real webview's IPC callback registry, which the mock harness does not construct — so
  the `attach_session` → event-stream path is owned by the E2E suite (which runs the
  entire app over the real WebView2 Channel: the announcer, buffer, and too-big specs
  all assert events that arrived down it). This matches the spec's stated fallback
  ("if the mock runtime cannot carry a Channel, the E2E suite is the named owner").

- **The live region appends, is emptied on idle, and moved out of the navigation
  path.** Agreed in conversation 2026-07-20 while dry-running the manual checklist.
  Three frontend-only changes (no protocol or backend change):
  1. *Moved.* A live region must stay in the accessibility tree to be announced, so
     its text is reachable by browse-mode navigation; with the region between the
     results and the edit field, a screen reader arrowing from the last command
     heading to the edit field read stale announcement text as page content. The
     region moves to the end of the document, off the most-travelled path.
  2. *Appends instead of replacing.* The `fail` dry run exposed a clobber: the error
     output (`Auto`) and the immediately-following `command failed, exit code N` both
     went to one region via `replaceChildren`, so the failure overwrote the output
     before the screen reader spoke it — only the failure was heard. `AnnouncerDom`
     now appends each announcement as a distinct node; a polite region reads additions
     in order (default `aria-relevant` is "additions text"), so back-to-back messages
     are all spoken, in order. Throttling a genuine flood is not this layer's job —
     that is the backend babble guard (B1); the frontend says every announcement the
     backend produced. Whether *status* announcements belong in a region separate from
     *output* (with their own polite/assertive and ordering semantics) is deferred to
     a DESIGN open question tied to the B1 pacing work; the end-of-command failure
     status is naturally after-the-output, but patience/too-big/alt-screen are
     inherently mid-command and cannot wait for command end.
  3. *Emptied on idle.* A single restarting timer empties the region a short, tunable
     delay (1.5 s) after the last announcement, so a burst accumulates and is cleared
     only once it settles. Emptying is safe and silent: the accessibility event fires
     on mutation and the screen reader copies the text into its own speech queue, so
     clearing the DOM later cannot retract unspoken speech, and removals are not
     announced. Consequence accepted: region-only announcements (failure, too-big,
     patience) become unreviewable in-app after the delay — reviewing them belongs to a
     later "status lines written into the buffer" decision, not here.

  The `speech` scenario (a single utterance longer than the clear delay, with a
  countable closing marker) is added to make truncation, if it ever occurs, audible
  during the NVDA pass.

## Definition of done

Merged with green CI (all jobs, including E2E and the bindings drift step); the
fake backend and verdict rendering are on main; echo harness removed;
ARCHITECTURE.md, DESIGN.md, and ROADMAP.md updated in this PR; manual NVDA
checklist completed in the PR body with findings (if any) filed as A5.x roadmap
entries.
