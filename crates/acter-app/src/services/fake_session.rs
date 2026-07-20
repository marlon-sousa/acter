//! Service: `FakeSessionService` — a scripted `SessionApi` backend (decision 5). It
//! plays baked-in scenario shapes with delays drawn from the fake script config; it
//! never computes a verdict or a boundary. Each `submit_command` allocates the next
//! `CommandId`, spawns a thread that plays the scenario, and returns immediately — an
//! invoke never waits on the shell (ARCHITECTURE, IPC rules).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use acter_core::{CommandId, EventSink, ExitCode, ReadMode, SessionEvent, SessionId, SubmitAck};

use crate::entities::{DelayRange, FakeScript};

pub(crate) struct FakeSessionService {
    script: FakeScript,
    // The one attached sink (Phase 1 has a single session). Replaced on re-attach
    // (a webview reload re-establishes the Channel).
    sink: Mutex<Option<Arc<dyn EventSink>>>,
    // Correlation-id counter; starts at 1 so 0 never appears as a real command.
    next_id: AtomicU32,
}

impl FakeSessionService {
    pub(crate) fn new(script: FakeScript) -> Self {
        Self {
            script,
            sink: Mutex::new(None),
            next_id: AtomicU32::new(1),
        }
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
            thread::spawn(move || play(scenario, command_id, &script, &line, sink.as_ref()));
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
            _ => Self::Echo,
        }
    }
}

/// Plays one scenario to completion (or, for `forever`, indefinitely) through `sink`,
/// pacing with the configured delays. Runs on its own thread; scenarios are demuxed
/// downstream by `command_id`, so concurrent commands may interleave (decision 7).
fn play(scenario: Scenario, id: CommandId, script: &FakeScript, line: &str, sink: &dyn EventSink) {
    sink.send(SessionEvent::CommandStarted { command_id: id });
    match scenario {
        Scenario::Small => {
            sleep(script.small.output_delay);
            output(sink, id, "hello from acter", ReadMode::Auto);
            finished(sink, id, 0, ReadMode::Auto);
        }
        Scenario::Big => {
            sleep(script.big.output_delay);
            output(
                sink,
                id,
                &numbered_lines(script.big.line_count),
                ReadMode::TooBig,
            );
            sleep(script.big.finish_delay);
            finished(sink, id, 0, ReadMode::Auto);
        }
        Scenario::Fail => {
            sleep(script.fail.output_delay);
            output(
                sink,
                id,
                "error: the command reported a problem",
                ReadMode::Auto,
            );
            finished(sink, id, script.fail.exit_code, ReadMode::Auto);
        }
        Scenario::Slow => {
            for phase in ["phase one", "phase two", "phase three"] {
                sleep(script.slow.chunk_delay);
                output(sink, id, phase, ReadMode::Auto);
            }
            finished(sink, id, 0, ReadMode::Auto);
        }
        Scenario::Forever => {
            sleep(script.forever.chunk_delay);
            output(sink, id, "phase one", ReadMode::Auto);
            sleep(script.forever.chunk_delay);
            output(sink, id, "phase two", ReadMode::Auto);
            sleep(script.forever.patience_delay);
            sink.send(SessionEvent::CommandStillRunning { command_id: id });
            loop {
                sleep(script.forever.quiet_interval);
                output(sink, id, "still working", ReadMode::Quiet);
            }
        }
        Scenario::Nano => {
            sleep(script.nano.enter_delay);
            sink.send(SessionEvent::AltScreenEntered);
            sleep(script.nano.leave_delay);
            sink.send(SessionEvent::AltScreenLeft);
            finished(sink, id, 0, ReadMode::Quiet);
        }
        Scenario::Tail => {
            for k in 1..=script.tail.iterations {
                sleep(script.tail.interval);
                output(sink, id, &format!("tail line {k}"), ReadMode::Auto);
            }
            finished(sink, id, 0, ReadMode::Auto);
        }
        Scenario::Burst => {
            sleep(script.burst.flood_delay);
            output(
                sink,
                id,
                &numbered_lines(script.burst.flood_lines),
                ReadMode::TooBig,
            );
            for m in 1..=script.burst.iterations {
                sleep(script.burst.interval);
                output(sink, id, &format!("trickle {m}"), ReadMode::Auto);
            }
            finished(sink, id, 0, ReadMode::Auto);
        }
        Scenario::Echo => {
            output(sink, id, line, ReadMode::Auto);
            finished(sink, id, 0, ReadMode::Auto);
        }
    }
}

fn output(sink: &dyn EventSink, id: CommandId, text: &str, read_mode: ReadMode) {
    sink.send(SessionEvent::Output {
        command_id: id,
        text: text.to_owned(),
        read_mode,
    });
}

fn finished(sink: &dyn EventSink, id: CommandId, exit_code: i32, read_mode: ReadMode) {
    sink.send(SessionEvent::CommandFinished {
        command_id: id,
        exit_code: ExitCode(exit_code),
        read_mode,
    });
}

/// A block of `n` numbered lines as one chunk. Content is not pinned by the spec (only
/// announcement strings are); the count is what the too-big announcement reports.
fn numbered_lines(n: u32) -> String {
    (1..=n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Sleeps for a sampled delay. Equal bounds sleep exactly (deterministic — the
/// all-zero-delays test config never sleeps and never draws a random number); unequal
/// bounds draw a value in `[min_ms, max_ms]` for organic manual pacing.
fn sleep(range: DelayRange) {
    let ms = if range.max_ms <= range.min_ms {
        range.min_ms
    } else {
        let span = range.max_ms - range.min_ms;
        range.min_ms + jitter() % (span + 1)
    };
    if ms > 0 {
        thread::sleep(Duration::from_millis(ms));
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
            let mut events = self.events.lock().unwrap();
            while events.len() < n {
                events = self.grew.wait(events).unwrap();
            }
            events.clone()
        }
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

    #[test]
    fn submit_without_an_attached_sink_still_acks() {
        let service = FakeSessionService::new(instant_script());
        let ack = service.submit_command(SessionId(1), "small");
        assert_eq!(ack.command_id, CommandId(1));
    }
}
