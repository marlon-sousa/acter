//! Service: `FakeSessionService` — a scripted `SessionApi` backend (decision 5). It
//! plays baked-in scenario shapes with delays drawn from the fake script config; it
//! never computes a verdict or a boundary. Each `submit_command` allocates the next
//! `CommandId`, spawns a thread that plays the scenario, and returns immediately — an
//! invoke never waits on the shell (ARCHITECTURE, IPC rules).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use acter_core::{CommandId, EventSink, ExitCode, ReadMode, SessionEvent, SessionId, SubmitAck};

use crate::entities::{DelayRange, FakeScript};

/// The scripts currently playing, so `stop` can reach them. Shared with every playback
/// thread, which deregisters itself when its script ends however it ends.
type Running = Arc<Mutex<HashMap<CommandId, Arc<CancelToken>>>>;

pub(crate) struct FakeSessionService {
    script: FakeScript,
    // The one attached sink (Phase 1 has a single session). Replaced on re-attach
    // (a webview reload re-establishes the Channel).
    sink: Mutex<Option<Arc<dyn EventSink>>>,
    // Correlation-id counter; starts at 1 so 0 never appears as a real command.
    next_id: AtomicU32,
    running: Running,
}

impl FakeSessionService {
    pub(crate) fn new(script: FakeScript) -> Self {
        Self {
            script,
            sink: Mutex::new(None),
            next_id: AtomicU32::new(1),
            running: Running::default(),
        }
    }
}

/// Cancellation for one playing script (spec decision 8). Rust has no safe thread
/// cancellation and the scripts are mostly asleep, so a stop must *wake* them: every
/// scripted delay waits on `wake` rather than sleeping, and returns early when
/// `cancelled` is set. Both sides take `lock` around the flag, so a script cannot check
/// it and start waiting in the gap where a stop would be lost.
#[derive(Default)]
struct CancelToken {
    cancelled: AtomicBool,
    lock: Mutex<()>,
    wake: Condvar,
}

/// A script's delay was cut short by a stop; the scenario must end with
/// `CommandInterrupted`. Threaded through the scenario bodies with `?`.
struct Stopped;

impl CancelToken {
    fn cancel(&self) {
        let _guard = self.lock.lock().expect("cancel lock poisoned");
        self.cancelled.store(true, Ordering::SeqCst);
        self.wake.notify_all();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Waits up to `ms`, returning early the moment a stop arrives.
    fn wait(&self, ms: u64) -> Result<(), Stopped> {
        let guard = self.lock.lock().expect("cancel lock poisoned");
        if self.is_cancelled() {
            return Err(Stopped);
        }
        if ms == 0 {
            return Ok(());
        }
        let _unused = self
            .wake
            .wait_timeout_while(guard, Duration::from_millis(ms), |()| !self.is_cancelled())
            .expect("cancel lock poisoned");
        if self.is_cancelled() {
            Err(Stopped)
        } else {
            Ok(())
        }
    }
}

/// Cancels every running script except `own` (a `stop` never interrupts itself, spec
/// decision 6). Tokens are collected before any is cancelled: a woken thread wants the
/// registry lock to deregister itself, so holding it here would deadlock.
fn cancel_others(running: &Running, own: CommandId) {
    let tokens: Vec<Arc<CancelToken>> = running
        .lock()
        .expect("running lock poisoned")
        .iter()
        .filter(|(id, _)| **id != own)
        .map(|(_, token)| Arc::clone(token))
        .collect();
    for token in tokens {
        token.cancel();
    }
}

impl acter_core::SessionApi for FakeSessionService {
    fn attach_session(&self, _session: SessionId, sink: Arc<dyn EventSink>) {
        *self.sink.lock().expect("sink lock poisoned") = Some(sink);
    }

    fn submit_command(&self, _session: SessionId, line: &str) -> SubmitAck {
        let command_id = CommandId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let sink = self.sink.lock().expect("sink lock poisoned").clone();
        if let Some(sink) = sink {
            let scenario = Scenario::select(line);
            let script = self.script;
            let line = line.to_owned();
            let cancel = Arc::new(CancelToken::default());
            let running = Arc::clone(&self.running);
            running
                .lock()
                .expect("running lock poisoned")
                .insert(command_id, Arc::clone(&cancel));
            thread::spawn(move || {
                Playback {
                    id: command_id,
                    script: &script,
                    line: &line,
                    sink: sink.as_ref(),
                    cancel: &cancel,
                    running: &running,
                }
                .run(scenario);
                // Deregister however the script ended, so a later `stop` cannot reach a
                // command that is already over.
                running
                    .lock()
                    .expect("running lock poisoned")
                    .remove(&command_id);
            });
        }
        // No attached sink yet: the ack still returns (the invoke path is always
        // honored); with nothing to emit through, no scenario runs.
        SubmitAck { command_id }
    }
}

/// Which scripted shape a typed line selects (decision 4). Anything unrecognized
/// echoes, preserving the A1 manual-testing loop.
enum Scenario {
    Small,
    Big,
    Fail,
    Slow,
    Forever,
    Nano,
    Tail,
    Burst,
    Speech,
    Stop,
    Echo,
}

impl Scenario {
    fn select(line: &str) -> Self {
        match line {
            "small" => Self::Small,
            "big" => Self::Big,
            "fail" => Self::Fail,
            "slow" => Self::Slow,
            "forever" => Self::Forever,
            "nano" => Self::Nano,
            "tail" => Self::Tail,
            "burst" => Self::Burst,
            "speech" => Self::Speech,
            "stop" => Self::Stop,
            _ => Self::Echo,
        }
    }
}

/// One playing script and everything it needs. Grouped into a struct so the scenario
/// bodies below read as the script table in the spec rather than as parameter plumbing.
struct Playback<'a> {
    id: CommandId,
    script: &'a FakeScript,
    line: &'a str,
    sink: &'a dyn EventSink,
    cancel: &'a CancelToken,
    running: &'a Running,
}

impl Playback<'_> {
    /// Plays one scenario to completion (or, for `forever`, until stopped) through the
    /// sink, pacing with the configured delays. Runs on its own thread; scenarios are
    /// demuxed downstream by `command_id`, so concurrent commands may interleave
    /// (decision 7). A stop cuts the scenario short and closes it with
    /// `CommandInterrupted` instead of `CommandFinished` (A3.1 decision 4).
    fn run(&self, scenario: Scenario) {
        self.send(SessionEvent::CommandStarted {
            command_id: self.id,
        });
        if self.scenario(scenario).is_err() {
            self.send(SessionEvent::CommandInterrupted {
                command_id: self.id,
            });
        }
    }

    fn scenario(&self, scenario: Scenario) -> Result<(), Stopped> {
        let script = self.script;
        match scenario {
            Scenario::Small => {
                self.sleep(script.small.output_delay)?;
                self.output("hello from acter", ReadMode::Auto);
                self.finished(0, ReadMode::Auto);
            }
            Scenario::Big => {
                self.sleep(script.big.output_delay)?;
                self.output(&numbered_lines(script.big.line_count), ReadMode::TooBig);
                self.sleep(script.big.finish_delay)?;
                self.finished(0, ReadMode::Auto);
            }
            Scenario::Fail => {
                self.sleep(script.fail.output_delay)?;
                self.output("error: the command reported a problem", ReadMode::Auto);
                self.finished(script.fail.exit_code, ReadMode::Auto);
            }
            Scenario::Slow => {
                for phase in ["phase one", "phase two", "phase three"] {
                    self.sleep(script.slow.chunk_delay)?;
                    self.output(phase, ReadMode::Auto);
                }
                self.finished(0, ReadMode::Auto);
            }
            Scenario::Forever => {
                self.sleep(script.forever.chunk_delay)?;
                self.output("phase one", ReadMode::Auto);
                self.sleep(script.forever.chunk_delay)?;
                self.output("phase two", ReadMode::Auto);
                self.sleep(script.forever.patience_delay)?;
                self.send(SessionEvent::CommandStillRunning {
                    command_id: self.id,
                });
                loop {
                    self.sleep(script.forever.quiet_interval)?;
                    self.output("still working", ReadMode::Quiet);
                }
            }
            Scenario::Nano => {
                self.sleep(script.nano.enter_delay)?;
                self.send(SessionEvent::AltScreenEntered);
                self.sleep(script.nano.leave_delay)?;
                self.send(SessionEvent::AltScreenLeft);
                self.finished(0, ReadMode::Quiet);
            }
            Scenario::Tail => {
                for k in 1..=script.tail.iterations {
                    self.sleep(script.tail.interval)?;
                    self.output(&format!("tail line {k}"), ReadMode::Auto);
                }
                self.finished(0, ReadMode::Auto);
            }
            Scenario::Burst => {
                self.sleep(script.burst.flood_delay)?;
                self.output(&numbered_lines(script.burst.flood_lines), ReadMode::TooBig);
                for m in 1..=script.burst.iterations {
                    self.sleep(script.burst.interval)?;
                    self.output(&format!("trickle {m}"), ReadMode::Auto);
                }
                self.finished(0, ReadMode::Auto);
            }
            Scenario::Speech => {
                self.sleep(script.speech.output_delay)?;
                self.output(&counted_phrase(script.speech.word_count), ReadMode::Auto);
                self.finished(0, ReadMode::Auto);
            }
            Scenario::Stop => {
                // Stops every other running script and says nothing of its own: the
                // stopped commands' `CommandInterrupted` events do the talking, and with
                // nothing running this is silent (decision 6).
                cancel_others(self.running, self.id);
                self.finished(0, ReadMode::Quiet);
            }
            Scenario::Echo => {
                self.output(self.line, ReadMode::Auto);
                self.finished(0, ReadMode::Auto);
            }
        }
        Ok(())
    }

    /// Waits out one scripted delay, propagating a stop so the scenario body unwinds
    /// with `?` at whatever step it had reached.
    fn sleep(&self, range: DelayRange) -> Result<(), Stopped> {
        self.cancel.wait(sample(range))
    }

    fn send(&self, event: SessionEvent) {
        self.sink.send(event);
    }

    fn output(&self, text: &str, read_mode: ReadMode) {
        self.send(SessionEvent::Output {
            command_id: self.id,
            text: text.to_owned(),
            read_mode,
        });
    }

    fn finished(&self, exit_code: i32, read_mode: ReadMode) {
        self.send(SessionEvent::CommandFinished {
            command_id: self.id,
            exit_code: ExitCode(exit_code),
            read_mode,
        });
    }
}

/// A block of `n` numbered lines as one chunk. Content is not pinned by the spec (only
/// announcement strings are); the count is what the too-big announcement reports.
fn numbered_lines(n: u32) -> String {
    (1..=n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One long auto-read phrase with unmistakable start and end markers and `words`
/// numbered words between them. Spoken as a single utterance, it lets a manual NVDA
/// pass hear whether emptying the live region truncates queued speech: if the closing
/// marker is heard, nothing was lost, and if speech stops it stops on a numbered word
/// that names exactly how far it got.
fn counted_phrase(words: u32) -> String {
    let mut phrase = String::from("long announcement starting.");
    for i in 1..=words {
        phrase.push_str(&format!(" word {i}"));
    }
    phrase.push_str(". long announcement finished");
    phrase
}

/// Samples one delay in milliseconds. Equal bounds are exact (deterministic — the
/// all-zero-delays test config never waits and never draws a random number); unequal
/// bounds draw a value in `[min_ms, max_ms]` for organic manual pacing.
fn sample(range: DelayRange) -> u64 {
    if range.max_ms <= range.min_ms {
        range.min_ms
    } else {
        let span = range.max_ms - range.min_ms;
        range.min_ms + jitter() % (span + 1)
    }
}

/// A cheap, time-seeded pseudo-random `u64` for delay jitter. Randomness quality is
/// irrelevant (this only varies pacing so manual sessions feel organic), so this stays
/// dependency-free rather than pulling in `rand`.
fn jitter() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // xorshift64 to spread the low-entropy time bits across the whole word.
    let mut x = nanos | 1;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

#[cfg(test)]
mod tests {
    use std::sync::Condvar;

    use acter_core::SessionApi;

    use super::*;

    /// A recording `EventSink`. `wait_len(n)` blocks the test until at least `n`
    /// events have arrived, then snapshots them — so a playback thread with zero
    /// delays is observed deterministically. With `park_at` set, `send` parks the
    /// producer thread forever once that many events are recorded: this is how the
    /// endless `forever` scenario is pinned to a finite, deterministic prefix without
    /// a runaway thread.
    struct Recorder {
        events: Mutex<Vec<SessionEvent>>,
        grew: Condvar,
        park_at: Option<usize>,
        park_lock: Mutex<()>,
        never: Condvar,
    }

    impl Recorder {
        fn new(park_at: Option<usize>) -> Arc<Self> {
            Arc::new(Self {
                events: Mutex::new(Vec::new()),
                grew: Condvar::new(),
                park_at,
                park_lock: Mutex::new(()),
                never: Condvar::new(),
            })
        }

        fn wait_len(&self, n: usize) -> Vec<SessionEvent> {
            self.wait_until(|events| events.len() >= n)
        }

        /// Blocks until the recorded events satisfy `done`, then snapshots them. Stop
        /// tests need this rather than `wait_len`: two playback threads interleave, so
        /// the interesting condition is "this event has arrived", not a total count.
        fn wait_until(&self, mut done: impl FnMut(&[SessionEvent]) -> bool) -> Vec<SessionEvent> {
            let mut events = self.events.lock().unwrap();
            while !done(&events) {
                events = self.grew.wait(events).unwrap();
            }
            events.clone()
        }
    }

    /// The events belonging to one command, in order. Concurrent scenarios interleave
    /// in the recorder, so per-command filtering is what makes an exact assertion
    /// meaningful; cross-command ordering is deliberately not asserted.
    fn for_command(events: &[SessionEvent], id: u32) -> Vec<SessionEvent> {
        events
            .iter()
            .filter(|event| command_of(event) == Some(CommandId(id)))
            .cloned()
            .collect()
    }

    fn command_of(event: &SessionEvent) -> Option<CommandId> {
        match event {
            SessionEvent::CommandStarted { command_id }
            | SessionEvent::Output { command_id, .. }
            | SessionEvent::CommandFinished { command_id, .. }
            | SessionEvent::CommandInterrupted { command_id }
            | SessionEvent::CommandStillRunning { command_id } => Some(*command_id),
            SessionEvent::AltScreenEntered
            | SessionEvent::AltScreenLeft
            | SessionEvent::TitleChanged { .. }
            | SessionEvent::ConnectionChanged { .. } => None,
        }
    }

    fn interrupted(id: u32) -> SessionEvent {
        SessionEvent::CommandInterrupted {
            command_id: CommandId(id),
        }
    }

    fn still_running(id: u32) -> SessionEvent {
        SessionEvent::CommandStillRunning {
            command_id: CommandId(id),
        }
    }

    fn has(events: &[SessionEvent], event: &SessionEvent) -> bool {
        events.contains(event)
    }

    impl EventSink for Recorder {
        fn send(&self, event: SessionEvent) {
            let reached_cap = {
                let mut events = self.events.lock().unwrap();
                events.push(event);
                self.grew.notify_all();
                self.park_at.is_some_and(|cap| events.len() >= cap)
            };
            if reached_cap {
                // Park this producer thread: wait on a condvar that is never notified,
                // which releases `park_lock` while parked. The `events` lock is already
                // released, so `wait_len` still observes the recorded prefix.
                let guard = self.park_lock.lock().unwrap();
                let _unused = self.never.wait_while(guard, |()| true).unwrap();
            }
        }
    }

    fn service(script: FakeScript) -> (FakeSessionService, Arc<Recorder>) {
        service_with_cap(script, None)
    }

    fn service_with_cap(
        script: FakeScript,
        park_at: Option<usize>,
    ) -> (FakeSessionService, Arc<Recorder>) {
        let service = FakeSessionService::new(script);
        let recorder = Recorder::new(park_at);
        service.attach_session(SessionId(1), recorder.clone());
        (service, recorder)
    }

    /// The all-zero-delays config with the small iteration counts the table test
    /// needs, built by zeroing every delay on the built-in defaults so the playback
    /// threads run to completion instantly and deterministically.
    fn instant_script() -> FakeScript {
        let z = DelayRange::fixed(0);
        let mut s = FakeScript::default();
        s.small.output_delay = z;
        s.big.output_delay = z;
        s.big.finish_delay = z;
        s.fail.output_delay = z;
        s.slow.chunk_delay = z;
        s.forever.chunk_delay = z;
        s.forever.patience_delay = z;
        s.forever.quiet_interval = z;
        s.nano.enter_delay = z;
        s.nano.leave_delay = z;
        s.tail.iterations = 2;
        s.tail.interval = z;
        s.burst.flood_lines = 3;
        s.burst.iterations = 2;
        s.burst.flood_delay = z;
        s.burst.interval = z;
        s.speech.output_delay = z;
        s.speech.word_count = 3;
        s
    }

    fn started(id: u32) -> SessionEvent {
        SessionEvent::CommandStarted {
            command_id: CommandId(id),
        }
    }

    fn out(id: u32, text: &str, mode: ReadMode) -> SessionEvent {
        SessionEvent::Output {
            command_id: CommandId(id),
            text: text.to_owned(),
            read_mode: mode,
        }
    }

    fn done(id: u32, code: i32, mode: ReadMode) -> SessionEvent {
        SessionEvent::CommandFinished {
            command_id: CommandId(id),
            exit_code: ExitCode(code),
            read_mode: mode,
        }
    }

    #[test]
    fn each_scenario_plays_its_exact_event_sequence() {
        struct Case {
            line: &'static str,
            expected: Vec<SessionEvent>,
        }
        let cases = [
            Case {
                line: "small",
                expected: vec![
                    started(1),
                    out(1, "hello from acter", ReadMode::Auto),
                    done(1, 0, ReadMode::Auto),
                ],
            },
            Case {
                line: "big",
                expected: vec![
                    started(1),
                    out(1, &numbered_lines(40), ReadMode::TooBig),
                    done(1, 0, ReadMode::Auto),
                ],
            },
            Case {
                line: "fail",
                expected: vec![
                    started(1),
                    out(1, "error: the command reported a problem", ReadMode::Auto),
                    done(1, 2, ReadMode::Auto),
                ],
            },
            Case {
                line: "slow",
                expected: vec![
                    started(1),
                    out(1, "phase one", ReadMode::Auto),
                    out(1, "phase two", ReadMode::Auto),
                    out(1, "phase three", ReadMode::Auto),
                    done(1, 0, ReadMode::Auto),
                ],
            },
            Case {
                line: "nano",
                expected: vec![
                    started(1),
                    SessionEvent::AltScreenEntered,
                    SessionEvent::AltScreenLeft,
                    done(1, 0, ReadMode::Quiet),
                ],
            },
            Case {
                line: "tail",
                expected: vec![
                    started(1),
                    out(1, "tail line 1", ReadMode::Auto),
                    out(1, "tail line 2", ReadMode::Auto),
                    done(1, 0, ReadMode::Auto),
                ],
            },
            Case {
                line: "burst",
                expected: vec![
                    started(1),
                    out(1, &numbered_lines(3), ReadMode::TooBig),
                    out(1, "trickle 1", ReadMode::Auto),
                    out(1, "trickle 2", ReadMode::Auto),
                    done(1, 0, ReadMode::Auto),
                ],
            },
            Case {
                line: "speech",
                expected: vec![
                    started(1),
                    out(1, &counted_phrase(3), ReadMode::Auto),
                    done(1, 0, ReadMode::Auto),
                ],
            },
            Case {
                line: "unrecognized text",
                expected: vec![
                    started(1),
                    out(1, "unrecognized text", ReadMode::Auto),
                    done(1, 0, ReadMode::Auto),
                ],
            },
        ];

        for case in cases {
            let (service, recorder) = service(instant_script());
            let ack = service.submit_command(SessionId(1), case.line);
            assert_eq!(ack.command_id, CommandId(1), "scenario {}", case.line);
            let events = recorder.wait_len(case.expected.len());
            assert_eq!(events, case.expected, "scenario {}", case.line);
        }
    }

    #[test]
    fn forever_emits_the_patience_prefix_then_accumulates_quietly() {
        // Park after the deterministic prefix so the endless quiet loop cannot run away.
        let prefix = vec![
            started(1),
            out(1, "phase one", ReadMode::Auto),
            out(1, "phase two", ReadMode::Auto),
            SessionEvent::CommandStillRunning {
                command_id: CommandId(1),
            },
            out(1, "still working", ReadMode::Quiet),
        ];
        let (service, recorder) = service_with_cap(instant_script(), Some(prefix.len()));
        service.submit_command(SessionId(1), "forever");
        let events = recorder.wait_len(prefix.len());
        assert_eq!(events, prefix);
    }

    #[test]
    fn the_ack_precedes_any_event() {
        // The ack is returned synchronously from submit_command; events arrive on the
        // playback thread afterward. A non-instant config guarantees the first event
        // is not yet recorded when the ack returns.
        let mut script = instant_script();
        script.small.output_delay = DelayRange::fixed(50);
        let (service, recorder) = service(script);
        let ack = service.submit_command(SessionId(1), "small");
        assert_eq!(ack.command_id, CommandId(1));
        // CommandStarted may or may not have landed yet, but the Output (gated on the
        // 50ms delay) certainly has not.
        {
            let events = recorder.events.lock().unwrap();
            assert!(
                events.len() <= 1,
                "no output should precede the returned ack, saw: {events:?}"
            );
        }
        recorder.wait_len(3);
    }

    #[test]
    fn command_ids_increment_across_submissions() {
        let (service, recorder) = service(instant_script());
        let first = service.submit_command(SessionId(1), "small");
        recorder.wait_len(3);
        let second = service.submit_command(SessionId(1), "small");
        assert_eq!(first.command_id, CommandId(1));
        assert_eq!(second.command_id, CommandId(2));
    }

    /// `forever` parked on a long quiet interval: the script is asleep in
    /// `CancelToken::wait`, which is exactly the state a stop has to interrupt.
    fn parked_forever_script() -> FakeScript {
        let mut script = instant_script();
        script.forever.quiet_interval = DelayRange::fixed(60_000);
        script
    }

    #[test]
    fn stop_halts_a_running_scenario_with_command_interrupted() {
        let (service, recorder) = service(parked_forever_script());
        service.submit_command(SessionId(1), "forever");
        // CommandStillRunning is the last event before the quiet loop's first wait, so
        // seeing it means the script is now parked in that 60s delay — the stop lands
        // mid-scenario, on a sleeping thread.
        recorder.wait_until(|events| has(events, &still_running(1)));

        service.submit_command(SessionId(1), "stop");
        let events = recorder.wait_until(|events| has(events, &interrupted(1)));

        let forever = for_command(&events, 1);
        assert_eq!(
            forever.last(),
            Some(&interrupted(1)),
            "the stopped scenario must end on CommandInterrupted, saw: {forever:?}"
        );
        assert!(
            !forever
                .iter()
                .any(|e| matches!(e, SessionEvent::CommandFinished { .. })),
            "CommandInterrupted is terminal — no CommandFinished may follow: {forever:?}"
        );
    }

    #[test]
    fn stop_wakes_a_script_sleeping_on_a_long_delay() {
        // The all-zero config cannot prove this: with every delay at zero a script is
        // never asleep, so a broken wake-up would still pass. `tail` here sits in a
        // 60-second interval; the stop must land in a small fraction of that.
        let mut script = instant_script();
        script.tail.iterations = 3;
        script.tail.interval = DelayRange::fixed(60_000);
        let (service, recorder) = service(script);

        service.submit_command(SessionId(1), "tail");
        recorder.wait_until(|events| has(events, &started(1)));

        let start = std::time::Instant::now();
        service.submit_command(SessionId(1), "stop");
        recorder.wait_until(|events| has(events, &interrupted(1)));
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(5),
            "stop must wake the sleeping script, not wait out its 60s delay (took {elapsed:?})"
        );
    }

    #[test]
    fn stop_does_not_interrupt_itself() {
        let (service, recorder) = service(instant_script());
        service.submit_command(SessionId(1), "stop");
        let events = recorder.wait_len(2);

        assert_eq!(
            for_command(&events, 1),
            vec![started(1), done(1, 0, ReadMode::Quiet)],
            "stop opens and closes its own quiet block and nothing more"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SessionEvent::CommandInterrupted { .. })),
            "with nothing else running, stop interrupts nothing: {events:?}"
        );
    }

    #[test]
    fn stop_halts_every_running_command() {
        let (service, recorder) = service(parked_forever_script());
        service.submit_command(SessionId(1), "forever");
        service.submit_command(SessionId(1), "forever");
        recorder
            .wait_until(|events| has(events, &still_running(1)) && has(events, &still_running(2)));

        service.submit_command(SessionId(1), "stop");
        // Wait for stop's own block to close too, not just for the interruptions it
        // caused: command 3 finishes on its own playback thread, so waiting only on the
        // interruptions can snapshot between its started and finished events.
        let events = recorder.wait_until(|events| {
            has(events, &interrupted(1))
                && has(events, &interrupted(2))
                && has(events, &done(3, 0, ReadMode::Quiet))
        });

        assert_eq!(for_command(&events, 1).last(), Some(&interrupted(1)));
        assert_eq!(for_command(&events, 2).last(), Some(&interrupted(2)));
        assert_eq!(
            for_command(&events, 3),
            vec![started(3), done(3, 0, ReadMode::Quiet)],
            "the stop command itself is unaffected"
        );
    }

    #[test]
    fn a_completed_command_is_deregistered_and_not_interrupted() {
        let (service, recorder) = service(instant_script());
        service.submit_command(SessionId(1), "small");
        recorder.wait_until(|events| has(events, &done(1, 0, ReadMode::Auto)));

        service.submit_command(SessionId(1), "stop");
        let events = recorder.wait_until(|events| has(events, &done(2, 0, ReadMode::Quiet)));

        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SessionEvent::CommandInterrupted { .. })),
            "a finished command must be deregistered, so stop cannot emit a stray \
             CommandInterrupted for it: {events:?}"
        );
    }

    #[test]
    fn submit_without_an_attached_sink_still_acks() {
        let service = FakeSessionService::new(instant_script());
        let ack = service.submit_command(SessionId(1), "small");
        assert_eq!(ack.command_id, CommandId(1));
    }
}
