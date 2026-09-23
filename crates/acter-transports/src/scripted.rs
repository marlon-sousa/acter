//! Adapter: `ScriptedTransport` — the fake pipe, a [`Transport`] that carries a
//! [`FakeShell`]'s bytes instead of a process's.
//!
//! Every delay comes from the [`Clock`] port; nothing here may sleep or use `tokio::time`.

pub(crate) mod transcript;

use std::collections::VecDeque;
use std::sync::Arc;

use acter_core::{Clock, Transport, TransportError};
use tokio::select;
use tokio::spawn;
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::fake::{Chunking, FakeShell, Script, Submission, TranscriptShell};
use transcript::{DelayRange, Repeat};

pub use transcript::SessionTranscript;

const INTERRUPT: u8 = 0x03;

/// Fixed, so sampled delays replay identically every run.
const ROLL_SEED: u64 = 0x_5EED_AC7E_5EED_AC7E;

pub struct ScriptedTransport {
    shell: Option<Box<dyn FakeShell>>,
    chunking: Chunking,
    clock: Arc<dyn Clock>,
    /// `None` until [`Transport::start`].
    writes: Option<UnboundedSender<Vec<u8>>>,
    reads: Option<Sender<Vec<u8>>>,
    written: Vec<u8>,
    last_resize: Option<(u16, u16)>,
}

impl ScriptedTransport {
    pub fn new(transcript: SessionTranscript, clock: Arc<dyn Clock>) -> Self {
        Self::with_shell(
            Box::new(TranscriptShell::new(transcript)),
            Chunking::Whole,
            clock,
        )
    }

    pub fn with_shell(
        shell: Box<dyn FakeShell>,
        chunking: Chunking,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            shell: Some(shell),
            chunking,
            clock,
            writes: None,
            reads: None,
            written: Vec::new(),
            last_resize: None,
        }
    }

    pub fn written(&self) -> &[u8] {
        &self.written
    }

    pub fn last_resize(&self) -> Option<(u16, u16)> {
        self.last_resize
    }
}

impl Transport for ScriptedTransport {
    /// Must be called from within a tokio runtime.
    fn start(&mut self, bytes: Sender<Vec<u8>>) {
        let (writes, inbox) = unbounded_channel();
        self.writes = Some(writes);
        self.reads = Some(bytes.clone());
        let Some(shell) = self.shell.take() else {
            return;
        };
        let emitter = Emitter {
            shell,
            chunking: self.chunking,
            clock: Arc::clone(&self.clock),
            bytes,
            inbox,
            pending: Vec::new(),
            queued: VecDeque::new(),
            roll: ROLL_SEED,
        };
        spawn(emitter.run());
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        let writes = self.writes.as_ref().ok_or(TransportError::NotStarted)?;
        if self.reads.as_ref().is_some_and(Sender::is_closed) {
            return Err(TransportError::Closed);
        }

        self.written.extend_from_slice(bytes);
        writes
            .send(bytes.to_vec())
            .map_err(|_| TransportError::Closed)
    }

    fn interrupt(&mut self) -> Result<(), TransportError> {
        self.write(&[INTERRUPT])
    }

    fn resize(&mut self, columns: u16, screen_lines: u16) -> Result<(), TransportError> {
        self.last_resize = Some((columns, screen_lines));
        Ok(())
    }
}

struct Emitter {
    shell: Box<dyn FakeShell>,
    chunking: Chunking,
    clock: Arc<dyn Clock>,
    bytes: Sender<Vec<u8>>,
    inbox: UnboundedReceiver<Vec<u8>>,
    pending: Vec<u8>,
    queued: VecDeque<Submission>,
    roll: u64,
}

/// The reader let go: the session is over.
struct Gone;

enum Ran {
    Completed,
    Interrupted(Submission),
    Gone,
}

enum Waited {
    Elapsed,
    Interrupted(Submission),
    Gone,
}

impl Emitter {
    async fn run(mut self) {
        if self.greet().await.is_err() {
            return;
        }
        while let Some(submission) = self.next_submission().await {
            let mut current = submission;
            loop {
                let answer = self.shell.answer(&current);
                match self.play(&answer).await {
                    Ran::Gone => return,
                    Ran::Interrupted(next) => current = next,
                    Ran::Completed => {
                        if self.greet().await.is_err() {
                            return;
                        }
                        break;
                    }
                }
            }
        }
    }

    async fn greet(&mut self) -> Result<(), Gone> {
        let prompt = self.shell.greet();
        match self.play(&prompt).await {
            Ran::Completed => Ok(()),
            Ran::Interrupted(submission) => {
                self.queued.push_front(submission);
                Ok(())
            }
            Ran::Gone => Err(Gone),
        }
    }

    async fn play(&mut self, script: &Script) -> Ran {
        for delivery in script.deliveries() {
            let mut left = match delivery.repeat() {
                Repeat::Times(times) => Some(times),
                Repeat::Endless(_) => None,
            };
            while left.is_none_or(|times| times > 0) {
                match self.wait(delivery.delay()).await {
                    Waited::Elapsed => {}
                    Waited::Interrupted(submission) => return Ran::Interrupted(submission),
                    Waited::Gone => return Ran::Gone,
                }
                if self.send(delivery.bytes()).await.is_err() {
                    return Ran::Gone;
                }
                left = left.map(|times| times - 1);
            }
        }
        Ran::Completed
    }

    /// A zero delay arms no timer, so an instant delivery never waits on a clock tick.
    async fn wait(&mut self, delay: DelayRange) -> Waited {
        if delay.is_instant() {
            return Waited::Elapsed;
        }
        let roll = self.next_roll();
        let mut timer = self.clock.timer(delay.pick(roll));
        loop {
            // `recv` is cancel-safe, so a message the losing branch was waiting for is still
            // there next time round.
            let received = select! {
                () = &mut timer => None,
                message = self.inbox.recv() => Some(message),
            };
            match received {
                None => return Waited::Elapsed,
                Some(None) => return Waited::Gone,
                Some(Some(written)) => {
                    if let Some(interrupt) = self.take(written) {
                        return Waited::Interrupted(interrupt);
                    }
                }
            }
        }
    }

    async fn next_submission(&mut self) -> Option<Submission> {
        loop {
            if let Some(queued) = self.queued.pop_front() {
                return Some(queued);
            }
            let written = self.inbox.recv().await?;
            if let Some(interrupt) = self.take(written) {
                self.queued.push_front(interrupt);
            }
        }
    }

    fn take(&mut self, written: Vec<u8>) -> Option<Submission> {
        self.pending.extend_from_slice(&written);
        let submissions = self.shell.accept(&mut self.pending);
        let mut interrupt = None;
        for submission in submissions {
            if interrupt.is_none() && self.shell.interrupts(&submission) {
                interrupt = Some(submission);
            } else {
                self.queued.push_back(submission);
            }
        }
        interrupt
    }

    async fn send(&mut self, bytes: &[u8]) -> Result<(), Gone> {
        for read in self.chunking.cut(bytes) {
            self.bytes.send(read.to_vec()).await.map_err(|_| Gone)?;
        }
        Ok(())
    }

    fn next_roll(&mut self) -> u64 {
        let mut roll = self.roll;
        roll ^= roll << 13;
        roll ^= roll >> 7;
        roll ^= roll << 17;
        self.roll = roll;
        roll
    }
}

#[cfg(test)]
mod tests {
    use std::mem::take;
    use std::sync::Mutex;
    use std::time::Duration;

    use acter_core::Timer;
    use tokio::sync::mpsc::{Receiver, channel};
    use tokio::sync::oneshot;
    use tokio::task::yield_now;

    use crate::Unmarked;

    use super::*;

    const READS: usize = 256;

    #[derive(Default)]
    struct FakeClock {
        now: Mutex<Duration>,
        armed: Mutex<Vec<(Duration, oneshot::Sender<()>)>>,
        requested: Mutex<Vec<Duration>>,
    }

    impl FakeClock {
        fn set_now(&self, at: Duration) {
            *self.now.lock().expect("clock poisoned") = at;
        }

        fn advance_to(&self, at: Duration) {
            self.set_now(at);
            let mut armed = self.armed.lock().expect("timers poisoned");
            let (due, pending) = take(&mut *armed)
                .into_iter()
                .partition::<Vec<_>, _>(|(deadline, _)| *deadline <= at);
            *armed = pending;
            for (_, fire) in due {
                let _ = fire.send(());
            }
        }

        fn next_deadline(&self) -> Option<Duration> {
            self.armed
                .lock()
                .expect("timers poisoned")
                .iter()
                .map(|(deadline, _)| *deadline)
                .min()
        }

        fn requested(&self) -> Vec<Duration> {
            self.requested.lock().expect("requests poisoned").clone()
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            *self.now.lock().expect("clock poisoned")
        }

        fn timer(&self, after: Duration) -> Timer {
            self.requested
                .lock()
                .expect("requests poisoned")
                .push(after);
            let (fire, fired) = oneshot::channel();
            let deadline = self.now() + after;
            self.armed
                .lock()
                .expect("timers poisoned")
                .push((deadline, fire));
            Timer::new(fired)
        }
    }

    struct Session {
        transport: ScriptedTransport,
        reads: Receiver<Vec<u8>>,
        clock: Arc<FakeClock>,
    }

    impl Session {
        fn start(transcript: SessionTranscript) -> Self {
            Self::over(Box::new(TranscriptShell::new(transcript)), Chunking::Whole)
        }

        fn over(shell: Box<dyn FakeShell>, chunking: Chunking) -> Self {
            let clock = Arc::new(FakeClock::default());
            let mut transport = ScriptedTransport::with_shell(shell, chunking, clock.clone());
            let (sender, reads) = channel(READS);
            transport.start(sender);
            Self {
                transport,
                reads,
                clock,
            }
        }

        fn write(&mut self, text: &str) {
            self.transport
                .write(text.as_bytes())
                .expect("the session is open");
        }

        /// Deterministic only on the current-thread runtime `tokio::test` builds, where the
        /// loop is ready only when a fake timer fires or a write arrives.
        async fn reads(&mut self) -> Vec<Vec<u8>> {
            let mut reads = Vec::new();
            let mut quiet = 0;
            while quiet < 2 {
                yield_now().await;
                let mut produced = false;
                while let Ok(chunk) = self.reads.try_recv() {
                    reads.push(chunk);
                    produced = true;
                }
                quiet = if produced { 0 } else { quiet + 1 };
            }
            reads
        }

        /// One deadline at a time, because a delivery arms its successor only once it has fired.
        async fn advance_to(&mut self, at: u64) -> Vec<Vec<u8>> {
            let at = Duration::from_millis(at);
            let mut reads = self.reads().await;
            while let Some(next) = self.clock.next_deadline().filter(|next| *next <= at) {
                self.clock.advance_to(next);
                reads.extend(self.reads().await);
            }
            self.clock.set_now(at);
            reads
        }
    }

    fn texts(reads: &[Vec<u8>]) -> Vec<String> {
        reads
            .iter()
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
            .collect()
    }

    fn transcript(json: &str) -> SessionTranscript {
        SessionTranscript::parse(json).expect("the test transcript parses")
    }

    fn ticking(repeat: &str, every: u64) -> SessionTranscript {
        transcript(&format!(
            r#"{{
              "on_start": [{{ "payload": {{ "text": "> " }} }}],
              "rules": [
                {{
                  "match": "go",
                  "steps": [
                    {{
                      "delay": {{ "min_ms": {every}, "max_ms": {every} }},
                      "payload": {{ "text": "tick" }},
                      "repeat": {repeat}
                    }}
                  ]
                }}
              ],
              "default": {{ "steps": [] }}
            }}"#
        ))
    }

    #[tokio::test]
    async fn starting_emits_the_prompt_as_the_shell_would() {
        let mut session = Session::start(SessionTranscript::builtin());

        assert_eq!(
            texts(&session.reads().await),
            ["\x1b]133;A\x07", "acter> ", "\x1b]133;B\x07"],
            "prompt start, the prompt itself, then command-line start"
        );
    }

    #[tokio::test]
    async fn a_written_line_is_echoed_then_answered_then_the_prompt_returns() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;

        session.write("small\n");
        assert_eq!(
            texts(&session.reads().await),
            ["small\r\n", "\x1b]133;C\x07"],
            "the terminal echoes what was typed, then the command starts running"
        );

        assert_eq!(
            texts(&session.advance_to(100).await),
            [
                "hello from acter\r\n",
                "\x1b]133;D;0\x07",
                "\x1b]133;A\x07",
                "acter> ",
                "\x1b]133;B\x07"
            ],
            "output, the command ends, and the next prompt is drawn"
        );
    }

    #[tokio::test]
    async fn an_unrecognized_line_takes_the_default_rule() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;

        session.write("nothing scripted this\n");

        assert_eq!(
            texts(&session.reads().await),
            [
                "nothing scripted this\r\n",
                "\x1b]133;C\x07",
                "\x1b]133;D;0\x07",
                "\x1b]133;A\x07",
                "acter> ",
                "\x1b]133;B\x07"
            ],
            "echoed, opened and closed: A1's manual loop, with real boundaries"
        );
    }

    #[tokio::test]
    async fn the_delays_requested_are_exactly_the_transcripts() {
        let mut session = Session::start(transcript(
            r#"{
              "on_start": [{ "payload": { "text": "> " } }],
              "rules": [
                {
                  "match": "go",
                  "steps": [
                    { "payload": { "text": "instant" } },
                    { "delay": { "min_ms": 10, "max_ms": 10 }, "payload": { "text": "a" } },
                    { "delay": { "min_ms": 250, "max_ms": 250 }, "payload": { "text": "b" } }
                  ]
                }
              ],
              "default": { "steps": [] }
            }"#,
        ));
        let _prompt = session.reads().await;

        session.write("go\n");
        let _all = session.advance_to(1000).await;

        assert_eq!(
            session.clock.requested(),
            [Duration::from_millis(10), Duration::from_millis(250)],
            "an instant delivery asks the clock for nothing at all"
        );
    }

    #[tokio::test]
    async fn a_counted_repeat_delivers_exactly_that_many_times() {
        let mut session = Session::start(ticking("3", 10));
        let _prompt = session.reads().await;

        session.write("go\n");
        let _echo = session.reads().await;

        assert_eq!(
            texts(&session.advance_to(1000).await),
            ["tick", "tick", "tick", "> "],
            "three deliveries, then the prompt returns"
        );
        assert_eq!(texts(&session.advance_to(5000).await), [] as [String; 0]);
    }

    #[tokio::test]
    async fn an_endless_repeat_keeps_going() {
        let mut session = Session::start(ticking(r#""forever""#, 10));
        let _prompt = session.reads().await;

        session.write("go\n");
        let _echo = session.reads().await;

        assert_eq!(session.advance_to(100).await.len(), 10);
        assert_eq!(
            session.advance_to(200).await.len(),
            10,
            "and it is still going"
        );
    }

    #[tokio::test]
    async fn an_interrupting_rule_cancels_a_sequence_while_it_is_waiting() {
        let mut session = Session::start(transcript(
            r#"{
              "on_start": [{ "payload": { "text": "> " } }],
              "rules": [
                {
                  "match": "go",
                  "steps": [
                    {
                      "delay": { "min_ms": 1000, "max_ms": 1000 },
                      "payload": { "text": "far too late" }
                    }
                  ]
                },
                {
                  "match": "stop",
                  "interrupts": true,
                  "steps": [{ "payload": { "text": "^C\r\n" } }]
                }
              ],
              "default": { "steps": [] }
            }"#,
        ));
        let _prompt = session.reads().await;

        session.write("go\n");
        let _echo = session.reads().await;
        assert!(session.advance_to(500).await.is_empty(), "still waiting");

        session.write("stop\n");
        assert_eq!(
            texts(&session.reads().await),
            ["stop\r\n", "^C\r\n", "> "],
            "the interrupt is answered where the wait was, and the prompt returns"
        );

        assert!(
            session.advance_to(60_000).await.is_empty(),
            "the cancelled delivery must never arrive, however long anyone waits"
        );
    }

    #[tokio::test]
    async fn an_interrupt_byte_needs_no_line_ending() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;
        session.write("forever\n");
        let _echo = session.reads().await;
        let _running = session.advance_to(3000).await;

        session.write("\u{3}");

        let answer = texts(&session.reads().await);
        // The answer restores the normal screen first, so only its end is compared.
        assert!(
            answer.iter().any(|said| said.ends_with("^C\r\n")),
            "got: {answer:?}"
        );
        assert!(
            answer.contains(&"\x1b]133;D\x07".to_owned()),
            "an interrupted command ends with no exit code to report: {answer:?}"
        );
    }

    #[tokio::test]
    async fn interrupting_reaches_the_same_rule_a_written_control_byte_does() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;
        session.write("forever\n");
        let _echo = session.reads().await;
        let _running = session.advance_to(3000).await;

        session.transport.interrupt().expect("the session is open");

        let answer = texts(&session.reads().await);
        assert!(
            answer.iter().any(|said| said.ends_with("^C\r\n")),
            "got: {answer:?}"
        );
        assert!(
            answer.contains(&"\x1b]133;D\x07".to_owned()),
            "an interrupted command ends with no exit code to report: {answer:?}"
        );
        assert_eq!(
            session.transport.written(),
            b"forever\n\x03",
            "and a scripted session keeps every byte it was told, this one included"
        );
    }

    #[tokio::test]
    async fn the_pipe_cuts_a_delivery_and_the_shell_never_does() {
        let mut session = Session::over(Box::new(TranscriptShell::builtin()), Chunking::Bytes(1));

        let reads = session.reads().await;
        assert!(
            reads.iter().all(|read| read.len() == 1),
            "every read is one byte: {:?}",
            texts(&reads)
        );
        assert_eq!(
            texts(&reads).concat(),
            "\x1b]133;A\x07acter> \x1b]133;B\x07",
            "and not a byte of the prompt was lost or moved"
        );
    }

    #[tokio::test]
    async fn an_unmarked_shell_still_prompts_and_answers() {
        let mut session = Session::over(
            Box::new(Unmarked::new(TranscriptShell::builtin())),
            Chunking::Whole,
        );

        assert_eq!(
            texts(&session.reads().await),
            ["acter> "],
            "the prompt text is drawn and the markers around it are not"
        );

        session.write("small\n");
        let answered = texts(&session.advance_to(100).await).concat();
        assert_eq!(answered, "small\r\nhello from acter\r\nacter> ");
    }

    #[tokio::test]
    async fn a_write_before_the_session_starts_is_a_speakable_error() {
        let mut transport =
            ScriptedTransport::new(SessionTranscript::builtin(), Arc::new(FakeClock::default()));

        assert_eq!(transport.write(b"small\n"), Err(TransportError::NotStarted));
    }

    #[tokio::test]
    async fn a_write_after_the_session_ends_is_a_speakable_error() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;

        let Session {
            mut transport,
            reads,
            ..
        } = session;
        drop(reads);

        let error = transport
            .write(b"small\n")
            .expect_err("the session has ended");
        assert_eq!(error, TransportError::Closed);
        assert_eq!(
            error.to_string(),
            "The session has ended, so the text could not be sent."
        );
    }

    #[tokio::test]
    async fn starting_twice_ends_the_second_session_rather_than_forking_the_far_end() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;

        let (sender, mut second) = channel(READS);
        session.transport.start(sender);
        yield_now().await;

        assert_eq!(
            second.try_recv().ok(),
            None,
            "nothing is emitted to a channel with no far end behind it"
        );
        assert_eq!(
            session.transport.write(b"small\n"),
            Err(TransportError::Closed)
        );
    }

    #[tokio::test]
    async fn a_resize_is_accepted_and_recorded() {
        let mut session = Session::start(SessionTranscript::builtin());

        assert_eq!(session.transport.last_resize(), None);
        session
            .transport
            .resize(100, 30)
            .expect("a resize is accepted");
        assert_eq!(session.transport.last_resize(), Some((100, 30)));
    }

    #[tokio::test]
    async fn bytes_written_are_recorded_and_a_partial_line_submits_nothing() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;

        session.write("\x1b[1;1R");

        assert_eq!(session.transport.written(), b"\x1b[1;1R");
        assert!(
            session.reads().await.is_empty(),
            "an unterminated write is not a command"
        );
    }

    #[tokio::test]
    async fn a_line_written_while_a_sequence_runs_is_answered_after_it() {
        let mut session = Session::start(ticking("1", 100));
        let _prompt = session.reads().await;

        session.write("go\n");
        let _echo = session.reads().await;
        session.write("later\n");
        assert!(
            session.reads().await.is_empty(),
            "the first answer is still running"
        );

        assert_eq!(
            texts(&session.advance_to(100).await),
            ["tick", "> ", "later\r\n", "> "],
            "the running sequence finishes, then the typeahead is answered"
        );
    }
}
