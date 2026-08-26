//! Service: `Conversation` — one attempt to connect, as a question asked and an answer
//! waited for.
//!
//! It is the join between the two halves of B9's shape: it implements
//! [`SshQuestions`](crate::SshQuestions), which the SSH transport calls when it needs a
//! person, and it turns each call into a [`ConnectStep`] on a [`ConnectSink`] and then
//! **blocks until somebody answers**.
//!
//! **Blocking is the point, and where it happens is the whole design.** The connection
//! genuinely cannot continue until the question is answered, so something has to wait. What
//! must never wait is an invoke: Tauri runs a synchronous command on the main thread, so an
//! invoke parked on a dialog would be holding the thread that the answering invoke needs in
//! order to be dispatched — a deadlock at the exact moment the host-key dialog appears, not
//! a slow connection. So the waiting is done here, on a task the connection owns, and every
//! invoke returns at once.
//!
//! **No runtime, no I/O, no framework.** The parking is a `std` channel, which is why this
//! is a service in the domain rather than a controller in the app: the whole conversation —
//! a key refused, a password given, a second password after a wrong one, a dialog cancelled
//! — is testable here with two threads and no Tauri anywhere near it.
//!
//! **One attempt per conversation.** The id it carries is minted per attempt and travels on
//! every question, so an answer typed into a dialog the user has already abandoned finds no
//! question waiting and is dropped, rather than resolving whatever is in flight now. A
//! password is the worst possible value to deliver to the wrong question.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

use crate::{
    AttemptId, ConnectAnswer, ConnectQuestion, ConnectSink, ConnectStep, HostKeyAnswer,
    HostKeyQuestion, HostKeyState, PasswordQuestion, Secret, SshQuestions,
};

/// One attempt to connect, and the one question it may be waiting on.
pub struct Conversation {
    attempt: AttemptId,
    steps: Arc<dyn ConnectSink>,
    /// Where to deliver the answer to the question being asked right now, and `None` when
    /// nothing is being asked. Exactly one question is ever outstanding: the connection is
    /// a single sequence, and it is parked while it waits.
    waiting: Mutex<Option<Sender<ConnectAnswer>>>,
}

impl Conversation {
    /// A conversation for this attempt, reporting to this sink.
    pub fn new(attempt: AttemptId, steps: Arc<dyn ConnectSink>) -> Self {
        Self {
            attempt,
            steps,
            waiting: Mutex::new(None),
        }
    }

    /// Which attempt this is, for whoever routes answers back to it.
    pub fn attempt(&self) -> AttemptId {
        self.attempt
    }

    /// Reports the end of the attempt, whichever end it was.
    ///
    /// Here rather than at the call site so that the two terminal steps cannot be spelled
    /// differently by two callers: a frontend waiting for the conversation to end is
    /// watching for exactly these.
    pub fn finished(&self, outcome: Result<crate::Connected, String>) {
        self.steps.send(match outcome {
            Ok(connected) => ConnectStep::Arrived { connected },
            Err(why) => ConnectStep::Failed { why },
        });
    }

    /// Delivers an answer to whatever is being asked, and does nothing when nothing is.
    ///
    /// **A stale answer is dropped rather than applied.** The dialog that asked may have
    /// been abandoned, the attempt may have failed for another reason entirely, or this may
    /// be a second click on a button that was already pressed — and in each case there is
    /// nothing this answer was the answer *to*.
    pub fn answer(&self, answer: ConnectAnswer) {
        let waiting = self
            .waiting
            .lock()
            .expect("conversation lock poisoned")
            .take();
        if let Some(sender) = waiting {
            // A receive end that has gone means the connection stopped waiting — it failed,
            // or it was torn down. Nothing to do and nothing to report.
            let _ = sender.send(answer);
        }
    }

    /// Asks, and waits.
    ///
    /// Every way of not getting an answer is [`ConnectAnswer::GiveUp`]: a sender dropped
    /// because the attempt was torn down means nobody is going to answer, and the safe
    /// reading of "nobody answered" is the one that does not connect.
    fn ask(&self, question: ConnectQuestion) -> ConnectAnswer {
        let (sender, receiver) = channel();
        *self.waiting.lock().expect("conversation lock poisoned") = Some(sender);
        self.steps.send(ConnectStep::Asked {
            attempt: self.attempt,
            question,
        });
        receiver.recv().unwrap_or(ConnectAnswer::GiveUp)
    }
}

impl SshQuestions for Conversation {
    /// **Anything but an explicit "trust this" is a refusal** (spec B9, decision 3). The
    /// default is not a property of the dialog that happens to be in front of the user; it
    /// is here, where the answer is read.
    fn host_key(&self, question: HostKeyQuestion) -> HostKeyAnswer {
        let HostKeyQuestion {
            host,
            port,
            fingerprint,
            state,
            aside,
        } = question;
        let asked = ConnectQuestion::HostKey {
            host,
            port,
            fingerprint,
            recorded: match state {
                HostKeyState::Unknown => None,
                HostKeyState::Changed { recorded } => Some(recorded),
            },
            aside,
        };

        match self.ask(asked) {
            ConnectAnswer::Trust => HostKeyAnswer::Accept,
            ConnectAnswer::GiveUp | ConnectAnswer::Password { .. } => HostKeyAnswer::Refuse,
        }
    }

    fn password(&self, question: PasswordQuestion) -> Option<Secret> {
        let PasswordQuestion { host, user, again } = question;

        match self.ask(ConnectQuestion::Password { host, user, again }) {
            ConnectAnswer::Password { secret } => Some(secret),
            // Trusting is not a password. It can only arrive here as a stale answer to a
            // host-key question that is no longer being asked, and treating it as consent
            // to continue with no password would be inventing an answer nobody gave.
            ConnectAnswer::GiveUp | ConnectAnswer::Trust => None,
        }
    }

    fn tell(&self, sentence: &str) {
        self.steps.send(ConnectStep::Progress {
            said: sentence.to_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;
    use std::thread;
    use std::time::Duration;

    use super::*;
    use crate::{Connected, SessionId};

    /// Long enough that a loaded machine is not what fails a test, short enough that a
    /// genuine deadlock is reported as one rather than hanging the suite.
    const PATIENCE: Duration = Duration::from_secs(5);

    /// A sink that hands each step to the test as it arrives, so a test blocks on the step
    /// rather than sleeping and hoping.
    struct Watcher(Mutex<Sender<ConnectStep>>);

    impl Watcher {
        fn new() -> (Arc<Self>, Receiver<ConnectStep>) {
            let (sender, receiver) = channel();
            (Arc::new(Self(Mutex::new(sender))), receiver)
        }
    }

    impl ConnectSink for Watcher {
        fn send(&self, step: ConnectStep) {
            let _ = self.0.lock().unwrap().send(step);
        }
    }

    fn unknown_key() -> HostKeyQuestion {
        HostKeyQuestion {
            host: "acter-ssh".to_owned(),
            port: 2222,
            fingerprint: "SHA256:offered".to_owned(),
            state: HostKeyState::Unknown,
            aside: None,
        }
    }

    /// Asks on a thread of its own, so the test can answer from this one — which is exactly
    /// the shape production has: the connection waits on its own task and the answer arrives
    /// from an invoke.
    fn asking<T: Send + 'static>(
        conversation: Arc<Conversation>,
        ask: impl FnOnce(Arc<Conversation>) -> T + Send + 'static,
    ) -> thread::JoinHandle<T> {
        thread::spawn(move || ask(conversation))
    }

    #[test]
    fn a_question_goes_out_carrying_the_attempt_it_belongs_to() {
        let (watcher, steps) = Watcher::new();
        let conversation = Arc::new(Conversation::new(AttemptId(7), watcher));

        let asked = asking(Arc::clone(&conversation), |it| it.host_key(unknown_key()));
        let step = steps.recv_timeout(PATIENCE).expect("the question goes out");

        let ConnectStep::Asked { attempt, question } = step else {
            panic!("a question is asked: {step:?}");
        };
        assert_eq!(attempt, AttemptId(7));
        assert_eq!(
            question,
            ConnectQuestion::HostKey {
                host: "acter-ssh".to_owned(),
                port: 2222,
                fingerprint: "SHA256:offered".to_owned(),
                recorded: None,
                aside: None,
            }
        );

        conversation.answer(ConnectAnswer::Trust);
        assert_eq!(asked.join().unwrap(), HostKeyAnswer::Accept);
    }

    /// **The default is refusal, and it lives here** rather than in whichever dialog is in
    /// front of the user: giving up on the conversation does not connect.
    #[test]
    fn giving_up_on_a_host_key_refuses_it() {
        let (watcher, steps) = Watcher::new();
        let conversation = Arc::new(Conversation::new(AttemptId(1), watcher));

        let asked = asking(Arc::clone(&conversation), |it| it.host_key(unknown_key()));
        steps.recv_timeout(PATIENCE).expect("the question goes out");
        conversation.answer(ConnectAnswer::GiveUp);

        assert_eq!(asked.join().unwrap(), HostKeyAnswer::Refuse);
    }

    /// A changed key travels as the same question carrying what was on file, which is what
    /// makes it the serious sentence rather than the routine one.
    #[test]
    fn a_changed_key_carries_the_fingerprint_that_was_recorded() {
        let (watcher, steps) = Watcher::new();
        let conversation = Arc::new(Conversation::new(AttemptId(1), watcher));

        let asked = asking(Arc::clone(&conversation), |it| {
            it.host_key(HostKeyQuestion {
                state: HostKeyState::Changed {
                    recorded: "SHA256:was".to_owned(),
                },
                ..unknown_key()
            })
        });
        let step = steps.recv_timeout(PATIENCE).expect("the question goes out");

        let ConnectStep::Asked {
            question: ConnectQuestion::HostKey { recorded, .. },
            ..
        } = step
        else {
            panic!("a host key is asked about: {step:?}");
        };
        assert_eq!(recorded.as_deref(), Some("SHA256:was"));

        conversation.answer(ConnectAnswer::GiveUp);
        asked.join().unwrap();
    }

    #[test]
    fn a_password_reaches_the_connection_and_giving_up_does_not() {
        for (answer, expected) in [
            (
                ConnectAnswer::Password {
                    secret: Secret::new("hunter2"),
                },
                Some("hunter2"),
            ),
            (ConnectAnswer::GiveUp, None),
        ] {
            let (watcher, steps) = Watcher::new();
            let conversation = Arc::new(Conversation::new(AttemptId(1), watcher));

            let asked = asking(Arc::clone(&conversation), |it| {
                it.password(PasswordQuestion {
                    host: "acter-ssh".to_owned(),
                    user: "acter".to_owned(),
                    again: false,
                })
            });
            steps.recv_timeout(PATIENCE).expect("the question goes out");
            conversation.answer(answer);

            assert_eq!(asked.join().unwrap().as_ref().map(Secret::expose), expected);
        }
    }

    /// **An answer to a question nobody is asking is dropped**, which is what the attempt id
    /// is for: a dialog the user abandoned, or a button pressed twice, must not resolve
    /// whatever is in flight now.
    #[test]
    fn an_answer_with_no_question_waiting_changes_nothing() {
        let (watcher, steps) = Watcher::new();
        let conversation = Conversation::new(AttemptId(1), watcher);

        conversation.answer(ConnectAnswer::Trust);

        assert!(
            steps.try_recv().is_err(),
            "nothing was asked, so nothing happened"
        );
    }

    /// Progress is said as it happens, because a listener with no feedback cannot tell a
    /// slow network from a dead one.
    #[test]
    fn what_is_happening_is_said_while_it_happens() {
        let (watcher, steps) = Watcher::new();
        let conversation = Conversation::new(AttemptId(1), watcher);

        conversation.tell("Connecting to acter-ssh.");

        assert_eq!(
            steps.recv_timeout(PATIENCE).expect("it is said"),
            ConnectStep::Progress {
                said: "Connecting to acter-ssh.".to_owned()
            }
        );
    }

    /// Both endings are spelled in one place, so a frontend waiting for the conversation to
    /// end has exactly two things to watch for.
    #[test]
    fn an_attempt_ends_as_one_of_two_steps() {
        let (watcher, steps) = Watcher::new();
        let conversation = Conversation::new(AttemptId(1), watcher);

        conversation.finished(Ok(Connected {
            session: SessionId(3),
            label: "SSH: acter at acter-ssh".to_owned(),
        }));
        conversation.finished(Err("Acter could not reach acter-ssh.".to_owned()));

        assert!(matches!(
            steps.recv_timeout(PATIENCE).unwrap(),
            ConnectStep::Arrived { .. }
        ));
        assert!(matches!(
            steps.recv_timeout(PATIENCE).unwrap(),
            ConnectStep::Failed { .. }
        ));
    }

    /// A conversation whose far side went away does not hang the connection waiting on an
    /// answer that is never coming: it gives up, which is the safe reading.
    #[test]
    fn a_question_nobody_can_answer_gives_up_rather_than_waiting_forever() {
        let (watcher, steps) = Watcher::new();
        let conversation = Arc::new(Conversation::new(AttemptId(1), watcher));

        let asked = asking(Arc::clone(&conversation), |it| it.host_key(unknown_key()));
        steps.recv_timeout(PATIENCE).expect("the question goes out");
        // Whoever would answer is gone: the sender waiting inside is dropped with it.
        *conversation.waiting.lock().unwrap() = None;

        assert_eq!(asked.join().unwrap(), HostKeyAnswer::Refuse);
    }
}
