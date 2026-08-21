# Spec: PR B4 — the local transport

Agreed in conversation 2026-08-21. Lane 2, entry B4. Delivers `LocalPty`: the second
implementer of the `Transport` port, a real shell on a real Windows pseudoconsole, and
the first bytes in this product that nobody wrote down in advance.

## Why now / relation to the roadmap

`Transport` has existed since B3.5 and the whole pipeline above it has run against a
scripted far end since B6. So this is not a new seam — it is the second thing plugged
into one that already works, which is the shape the lane was built to reach: B4 and B5
change what is at the far end, rather than being the first moment the pipeline runs.

Two things filed here by earlier entries come due, and the roadmap pinned their answers
in conversation on 2026-08-21 before this spec was written. They are restated as
decisions 6, 7 and 8 rather than reopened.

## Design decisions this spec makes

1. **Placement: `crates/acter-transports/src/local.rs`, role adapter.**

   `LocalPty`, beside `ScriptedTransport` and behind the same port, re-exported from the
   crate facade. `portable-pty` 0.9 is the dependency ARCHITECTURE already named; it
   wraps the ConPTY API and gives the same shape on other platforms, which matters
   because none of this file's logic is Windows-specific even though phase 1 only ships
   Windows.

2. **It names no shell.** The constructor takes the program and arguments to spawn.

   Which shell to run, and what to inject into it, is `ShellAdapter`'s and therefore
   B5's — DESIGN's transport-versus-shell criterion, the same one that put `interrupt`
   on this port and EOF on the other. A transport that reached for "powershell.exe"
   would be making a shell decision at the transport seam, and the SSH implementer next
   to it would then have to make the same decision differently.

3. **The reader is a thread, and one read is one send.**

   The port's own doc blesses this: a blocking PTY read gets a dedicated thread feeding
   the session channel, which is exactly the strategy knowledge that differs between
   implementers. The thread reads into a fixed buffer and sends precisely what it read,
   never merging two reads to make the stream tidier — chunk boundaries are what DESIGN's
   reliability cases are about, and this is the implementer where they finally become
   real rather than simulated.

   `Sender::blocking_send` is the channel call, because the thread is not async and must
   not be. Back-pressure is honest here: if the domain is behind, the shell is slowed
   down, which is what the bounded read channel in `SessionService` already decided.

   **The session ends when the channel sender is dropped**, which happens when the read
   returns zero bytes or fails. Not an error variant: a shell that exits is not a
   failure, and the port models the end of a session as its channel closing.

   **Amendment, forced by the first run of the real-shell suite.** That is not enough on
   its own, and the gap was a defect rather than a detail: a pseudoconsole stays open, and
   its reader stays blocked, for as long as anything holds the master — a child exiting
   does not close it. So `cmd /C echo done` exited and the read channel stayed open, which
   in the app would be a session that goes on accepting commands with nothing at the far
   end and says nothing about it. What ships is therefore a second thread that waits on
   the shell and then **drops the master**, which closes the pseudoconsole and makes the
   blocking read return; the adapter keeps a `ChildKiller` rather than the child itself, so
   tearing the session down still kills the shell. `resize` after that answers `Closed`
   rather than reporting success against a pseudoconsole that no longer exists.

4. **`interrupt` writes `0x03` into the data stream.**

   That is what B6 said this implementer would do, and it is the reason `interrupt` is a
   port method rather than bytes a service computes: over ConPTY an interrupt travels
   *in* the stream, over SSH it is a channel request that travels outside it. The
   pseudoconsole turns the control byte into a console control event for the attached
   process, which is what makes it an interrupt rather than a character.

5. **Errors are the ones the port already has, and they stay speakable.** `NotStarted`
   before `start`, `Closed` once the far end is gone, and `Failed { detail }` carrying
   the operating system's own words as its own sentence. This is the first transport
   whose failures are real, so it is also the first place the `Failed` variant earns its
   `detail` field.

6. **Read timing is measured, not modelled.** (Pinned 2026-08-21.)

   No timing model lands speculatively. This PR instruments the real reader and records
   what it observed — a build log, a large directory listing, a program that dribbles —
   as a finding in the PR body: the distribution of gaps between reads, and how they
   compare to `PacingConfig::quiescence` (500 ms).

   The expectation to prove or disprove is that the pseudoconsole hands over whatever is
   in its buffer, so gaps cluster *between* program writes — which `DelayRange` already
   models — and gaps within one write are microseconds. **If that is what the measurement
   says, this PR adds no model and writes down why**, which is a result and not a dodge.

   If it earns one, the gap goes on the pipe as `read_gap: DelayRange` beside `chunking`
   (decision 7), and lands in this PR with the numbers that justified it.

   **Result: it did not earn one, and none landed.** Measured on the development machine:
   `dir /s C:\Windows\System32` gave 25,407 reads and 2.4 MB with a median gap of 188
   microseconds, a p99 of 348 and a maximum of 3.2 ms — no gap came near the 500 ms
   quiescence window. `ping -n 4 127.0.0.1` gave 8 reads with three gaps over a second,
   and those are the pauses *between* the program's own writes, which `DelayRange` already
   models. The expectation held, `Chunking` is untouched, and the reason is written down
   rather than the model being added "while we are here".

   **And the measurement was sharpened to answer the policy question it was really about.**
   It also counts gaps at or over the quiescence window that arrive while the text so far
   ends mid-line — the shape that would make speech read half a sentence. Over both
   streams that count was **zero**: a local shell writes whole lines, and its long gaps
   fall between lines rather than inside one. So the DESIGN question stays open and is not
   urgent, and the place to revisit it is SSH, where a network can split a write that a
   pseudoconsole does not.

7. **`Chunking` stays pure.** (Pinned 2026-08-21.)

   It keeps its meaning — where the cuts fall — and gains no clock. Any gap belongs to
   the pipe, which already holds the `Clock` and the seeded roll, and is expressed with
   the existing `DelayRange` for its sampling, its `is_instant()` fast path and its
   deterministic roll. Defaulting to instant keeps every existing fixture meaning exactly
   what it means today.

   A clock inside the policy would cost the purity its own module doc claims in its last
   line, and that purity is what makes `Chunking` a free dimension every fixture can be
   crossed with.

8. **The fake's delivery stays atomic, and the truncated sequence gets a test.** (Pinned
   2026-08-21.)

   An interrupt still cancels between deliveries and never inside one. Making a delivery
   divisible would give every interrupt fixture a timing-dependent outcome, which is what
   B3.6 deliberately removed from this crate.

   The question a real interrupt genuinely raises is different, and it is answered here:
   a program killed mid-write can emit `ESC ] 1 3 3 ; D` with no terminator, followed by
   the shell's `^C` and a new prompt beginning with its own `A`. A parser that accumulated
   to the next terminator would swallow both, and the audible result would be a command
   that never ends followed by a prompt that never appears. There is no coverage of this
   today — `acter-term/tests/vte_unhandled_osc.rs` covers unrecognized but *complete* OSC
   sequences — so this PR adds one: an unterminated OSC 133 followed by `^C` and the next
   prompt still recognizes that prompt's `A`. If the parser swallows it, the fix is
   bounded OSC accumulation in `acter-term` and it rides here.

   **Result: vte recovers, and nothing needed fixing.** The next prompt's `A`, `B` and `C`
   are all recognized, whole or arriving a byte at a time, so what landed is two tests that
   pin the recovery rather than a change to the engine.

9. **The app can select a real shell, and it is not the default.**

   `ACTER_SHELL` names a program for the container to spawn on a `LocalPty`; unset keeps
   the scripted far end exactly as it is. That is one branch in the composition root,
   beside the `ACTER_TRANSCRIPT` table that already exists for the same reason: a manual
   accessibility run needs a way to say which session it is testing.

   **It is not the profile machinery and it is not convergence.** Convergence still owns
   making a real transport the default and putting it behind a profile. What this buys
   now is that B4 can be heard rather than only tested — and what will be heard is a real
   PowerShell with no shell integration, which is DESIGN's reliability case 2 running
   against a real shell for the first time. Every command in such a session degrades: no
   boundaries, no auto-read, output in the buffer for review, patience announcements
   intact. That is the honest state of the product until B5 injects the markers.

10. **The harness answers device queries, because a real shell will not start without an
    answer.** (Found while writing the tests, not designed.)

    The first thing `cmd.exe` puts on the wire is `ESC [ 6 n` — a cursor-position report
    request — and it draws no prompt at all until something answers. The first version of
    this suite drove `LocalPty` alone, and every test in it timed out having seen exactly
    those four bytes.

    Nothing needed fixing: B3 decision 4 built the reply path for precisely this, and
    `SessionService` already writes `TerminalEngine::take_replies()` back. What it changes
    is the *tests*, which now run their reads through a real engine and write its replies
    back, the way the service does. It is recorded here because it is the first evidence
    that a **shell** is one of the programs that waits forever — B3 argued the case from
    programs in general — and because the next person to drive a transport by hand will
    otherwise lose an hour to it.

11. **The real-shell tests run in their own CI job.**

    They spawn a process, so they are `#[ignore]`d and invisible to
    `cargo test --workspace`, and a new `real-shell (Windows)` job runs them explicitly.
    Keeping them out of the default run is what stops the fast suite from depending on
    what is installed on the machine, and giving them a job is what stops them from
    rotting unrun.

## Files touched

- `crates/acter-transports/Cargo.toml` — `portable-pty` 0.9.
- `crates/acter-transports/src/local.rs` — new: `LocalPty`, role adapter.
- `crates/acter-transports/src/lib.rs` — the re-export.
- `crates/acter-transports/tests/local_pty.rs` — new: the real-shell tests and the read
  timing measurement, all `#[ignore]`d.
- `crates/acter-term/src/alacritty_engine.rs` — the two truncated-sequence tests. No fix
  was needed: vte recovers.
- `crates/acter-app/src/container.rs` — the `ACTER_SHELL` branch.
- `.github/workflows/ci.yml` — the `real-shell (Windows)` job.
- `docs/ROADMAP.md` — entry 22 flipped to Done; the read-timing finding recorded.

## Tests

Unit, in `local.rs`, with no process spawned:

- `write` and `interrupt` before `start` answer `NotStarted`, in whole sentences.
- `interrupt` writes exactly one byte and that byte is `0x03`.
- Writing after the far end is gone answers `Closed`, and any other failure carries the
  operating system's own words as a sentence of their own.
- What the session holds is dropped once the shell has been waited for — the amendment to
  decision 3, pinned without a process by making the waiting generic over what it closes.

Real-shell, in `tests/local_pty.rs`, `#[ignore]`d:

- A shell starts, says something, and the reads arrive; the session ends when the shell
  exits, and it ends by the channel closing rather than by an error.
- A submitted line comes back echoed, which is the first time the echo this product reads
  headings from (B6.1) is produced by something nobody scripted.
- An interrupt stops a command that would not have ended on its own.
- A resize is accepted and the shell sees the new width.
- **The measurement**: a command producing sustained output, with the gap between reads
  recorded and printed, plus the largest gap seen. Its output goes in the PR body.

Engine, in `acter-term`:

- An unterminated OSC 133 `D`, then `^C`, then a full prompt sequence: the prompt's `A`
  is still recognized and the text between is not swallowed.

## Acceptance criteria

1. `cargo test --workspace` green; `cargo clippy --workspace --all-targets` clean under
   `-D warnings`; `cargo fmt --check` clean; `npm test` green; the e2e suite green.
2. The real-shell job passes on CI, and `cargo test --workspace` spawns no process.
3. `acter-core` gains no dependency, and `LocalPty` names no shell.
4. Module role declared on the first line of the new module's `//!` comment; the
   visibility ladder holds.
5. The read-timing measurement is in the PR body, with the decision it produced —
   a `read_gap` with numbers, or an explicit "no model, and here is why".
6. **Accessibility checklist in the PR body**: a real shell heard through `ACTER_SHELL`,
   including that its lack of shell integration is announced once and that its output
   still reaches the buffer.

## What the manual pass found, and what became of it

Three defects, none of them reachable against a scripted far end. Two are fixed here; two
are filed, because they are not this component's.

- **A bare line feed is not Enter** — fixed here. `SessionService` wrote `"{line}
"`, and
  a real shell echoes that and never runs the line, so every command in a real session was
  accepted and silently did nothing. The domain now writes a carriage return, which is
  what a terminal sends. Pinned at both levels: the service asserts the bytes, and the
  real-shell suite asserts that a line feed is echoed and *not run* while a carriage
  return runs.
- **A shell that exited did not end the session** — fixed here, decision 3's amendment.
- **An interrupt does not stop a running program** — filed as roadmap **B4.1**, and it is
  the serious one: the layers above treat the interrupt as a boundary, so Acter says
  `command stopped` while the program keeps going. `LocalPty::interrupt` still ships,
  because the byte does reach the far end and the port needs an implementer; what does not
  ship is any claim that it works. The requirement is written as a test, skipped by name
  in CI.
- **Text that scrolled away is emitted twice** — filed as roadmap **B4.2**. It is the
  pump's per-line record being cleared at a command boundary while the engine still
  considers the row live, which is `acter-core`'s to answer rather than this adapter's.

## Out of scope

- **Making a real transport the default, and profiles.** Convergence's.
- **Shell integration.** B5's: this PR's real session is deliberately an unintegrated
  one, and that is the honest state until the markers are injected.
- **EOF, and the null `ShellAdapter`.** B5's, for the reason B6 decision 6 gave.
- **Window-driven resize.** `SessionApi` still exposes no resize path; the port method is
  implemented and unit-tested, and the surface that would call it is not this PR's.
- **Whether quiescence should flush a trailing partial line.** The measurement in
  decision 6 is what should inform it, and it is a DESIGN question about the auto-read
  policy rather than a transport one. Recorded as open, deliberately unanswered here.
