//! Service: `Conversation` — one attempt to connect, as a question asked and an answer
//! waited for.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

use crate::{
    AttemptId, ConnectAnswer, ConnectQuestion, ConnectQuestions, ConnectSink, ConnectStep,
    HostKeyAnswer, HostKeyQuestion, HostKeyState, IF_YOU_SKIP, PasswordQuestion, ProgramAnswer,
    ProgramQuestion, Secret, SetupAnswer, SetupQuestion, SshQuestions,
};

/// Its question methods block until `answer` is called, so they must never run on the
/// thread that delivers answers.
pub struct Conversation {
    attempt: AttemptId,
    steps: Arc<dyn ConnectSink>,
    /// `None` when nothing is being asked.
    waiting: Mutex<Option<Sender<ConnectAnswer>>>,
}

impl Conversation {
    pub fn new(attempt: AttemptId, steps: Arc<dyn ConnectSink>) -> Self {
        Self {
            attempt,
            steps,
            waiting: Mutex::new(None),
        }
    }

    pub fn attempt(&self) -> AttemptId {
        self.attempt
    }

    pub fn finished(&self, outcome: Result<crate::Connected, String>) {
        self.steps.send(match outcome {
            Ok(connected) => ConnectStep::Arrived { connected },
            Err(why) => ConnectStep::Failed { why },
        });
    }

    pub fn answer(&self, answer: ConnectAnswer) {
        let waiting = self
            .waiting
            .lock()
            .expect("conversation lock poisoned")
            .take();
        if let Some(sender) = waiting {
            // Err means the connection stopped waiting, and there is nothing to report.
            let _ = sender.send(answer);
        }
    }

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
            ConnectAnswer::GiveUp
            | ConnectAnswer::Password { .. }
            | ConnectAnswer::StartAnyway
            | ConnectAnswer::SetUpSession { .. } => HostKeyAnswer::Refuse,
        }
    }

    fn password(&self, question: PasswordQuestion) -> Option<Secret> {
        let PasswordQuestion { host, user, again } = question;

        match self.ask(ConnectQuestion::Password { host, user, again }) {
            ConnectAnswer::Password { secret } => Some(secret),
            ConnectAnswer::GiveUp
            | ConnectAnswer::Trust
            | ConnectAnswer::StartAnyway
            | ConnectAnswer::SetUpSession { .. } => None,
        }
    }

    fn tell(&self, sentence: &str) {
        self.steps.send(ConnectStep::Progress {
            said: sentence.to_owned(),
        });
    }
}

impl ConnectQuestions for Conversation {
    fn unverified(&self, question: ProgramQuestion) -> ProgramAnswer {
        let ProgramQuestion {
            label,
            program,
            verdict,
        } = question;
        let asked = ConnectQuestion::Unverified {
            label,
            program,
            said: verdict.said(),
            signer: verdict.signer(),
        };

        match self.ask(asked) {
            ConnectAnswer::StartAnyway => ProgramAnswer::Start,
            ConnectAnswer::GiveUp
            | ConnectAnswer::Trust
            | ConnectAnswer::Password { .. }
            | ConnectAnswer::SetUpSession { .. } => ProgramAnswer::DoNotStart,
        }
    }

    fn set_up_session(&self, question: SetupQuestion) -> SetupAnswer {
        let asked = ConnectQuestion::SetUpSession {
            detected: question.detected(),
            offer: question.offer(),
            command: question.command().to_owned(),
            refusal: IF_YOU_SKIP.to_owned(),
            shell: question.shell,
        };

        match self.ask(asked) {
            ConnectAnswer::SetUpSession { remember } => SetupAnswer::SetUp { remember },
            ConnectAnswer::GiveUp
            | ConnectAnswer::Trust
            | ConnectAnswer::Password { .. }
            | ConnectAnswer::StartAnyway => SetupAnswer::Skip,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;
    use std::thread;
    use std::time::Duration;

    use super::*;
    use crate::{Connected, Fault, LineOwner, SessionId, Verdict};

    const PATIENCE: Duration = Duration::from_secs(5);

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

    #[test]
    fn giving_up_on_a_host_key_refuses_it() {
        let (watcher, steps) = Watcher::new();
        let conversation = Arc::new(Conversation::new(AttemptId(1), watcher));

        let asked = asking(Arc::clone(&conversation), |it| it.host_key(unknown_key()));
        steps.recv_timeout(PATIENCE).expect("the question goes out");
        conversation.answer(ConnectAnswer::GiveUp);

        assert_eq!(asked.join().unwrap(), HostKeyAnswer::Refuse);
    }

    #[test]
    fn a_file_that_did_not_verify_is_asked_about_in_the_domains_own_words() {
        let (watcher, steps) = Watcher::new();
        let conversation = Arc::new(Conversation::new(AttemptId(3), watcher));

        let asked = asking(Arc::clone(&conversation), |it| {
            it.unverified(ProgramQuestion {
                label: "PowerShell 7".to_owned(),
                program: r"C:\tools\pwsh\pwsh.exe".to_owned(),
                verdict: Verdict::Untrusted {
                    fault: Fault::UntrustedRoot {
                        signer: Some("Contoso Corporation".to_owned()),
                    },
                },
            })
        });
        let step = steps.recv_timeout(PATIENCE).expect("the question goes out");

        let ConnectStep::Asked {
            attempt,
            question:
                ConnectQuestion::Unverified {
                    label,
                    program,
                    said,
                    signer,
                },
        } = step
        else {
            panic!("a file is asked about: {step:?}");
        };
        assert_eq!(attempt, AttemptId(3));
        assert_eq!(label, "PowerShell 7");
        assert_eq!(program, r"C:\tools\pwsh\pwsh.exe");
        assert_eq!(signer.as_deref(), Some("Contoso Corporation"));
        assert!(
            said.contains("does not trust"),
            "the verdict's own sentence travels: {said}"
        );

        conversation.answer(ConnectAnswer::StartAnyway);
        assert_eq!(asked.join().unwrap(), ProgramAnswer::Start);
    }

    #[test]
    fn nothing_but_saying_so_starts_a_file_that_did_not_verify() {
        for answer in [
            ConnectAnswer::GiveUp,
            ConnectAnswer::Trust,
            ConnectAnswer::Password {
                secret: Secret::new("hunter2"),
            },
        ] {
            let (watcher, steps) = Watcher::new();
            let conversation = Arc::new(Conversation::new(AttemptId(1), watcher));

            let asked = asking(Arc::clone(&conversation), |it| {
                it.unverified(ProgramQuestion {
                    label: "PowerShell 7".to_owned(),
                    program: r"C:\tools\pwsh\pwsh.exe".to_owned(),
                    verdict: Verdict::Untrusted {
                        fault: Fault::NotSigned,
                    },
                })
            });
            steps.recv_timeout(PATIENCE).expect("the question goes out");
            conversation.answer(answer);

            assert_eq!(asked.join().unwrap(), ProgramAnswer::DoNotStart);
        }
    }

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

    #[test]
    fn an_attempt_ends_as_one_of_two_steps() {
        let (watcher, steps) = Watcher::new();
        let conversation = Conversation::new(AttemptId(1), watcher);

        conversation.finished(Ok(Connected {
            session: SessionId(3),
            label: "SSH: acter at acter-ssh".to_owned(),
            note: None,
            limit_explained: false,
            saved_as: None,
            line_owner: LineOwner::FarEnd,
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

    #[test]
    fn a_question_nobody_can_answer_gives_up_rather_than_waiting_forever() {
        let (watcher, steps) = Watcher::new();
        let conversation = Arc::new(Conversation::new(AttemptId(1), watcher));

        let asked = asking(Arc::clone(&conversation), |it| it.host_key(unknown_key()));
        steps.recv_timeout(PATIENCE).expect("the question goes out");
        *conversation.waiting.lock().unwrap() = None;

        assert_eq!(asked.join().unwrap(), HostKeyAnswer::Refuse);
    }
}
