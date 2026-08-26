//! Controller (orchestrator): `Connecting` — one attempt to connect, from the invoke that
//! starts it to the answer that lets it finish.
//!
//! **It exists because of a Tauri fact, and the fact is worth stating plainly.** A
//! synchronous `#[tauri::command]` runs on the **main thread**. Starting an SSH far end
//! blocks until a person has decided about a host key and typed a password. So a router
//! that called `use_profile` directly would hold the main thread across a dialog, and the
//! invoke carrying the *answer* could never be dispatched — a deadlock at the exact moment
//! the dialog appears, not a slow connection. Everything in this file follows from that:
//! the work goes on a task, the invokes return at once, and the two are joined by an
//! attempt id.
//!
//! **What is here and what is not.** The waiting, the parking and the whole conversation
//! are `acter_core::Conversation`'s, where they are tested with two threads and no
//! framework. What is here is the part that genuinely needs Tauri: spawning onto the
//! runtime, and remembering which attempt an answering invoke belongs to.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use acter_core::{
    AttemptId, ConnectAnswer, ConnectApi, ConnectSink, Conversation, ProfileId, SshQuestions,
};

/// The attempts in flight, and the means to start another.
pub(crate) struct Connecting {
    connect: Arc<dyn ConnectApi>,
    /// Which conversations can still be answered.
    ///
    /// **A map rather than one slot**, because a user who gives up on a dialog and starts
    /// again has two attempts alive for a moment. Keyed by the id every question carries,
    /// so an answer can only ever reach the conversation that asked.
    live: Mutex<HashMap<AttemptId, Arc<Conversation>>>,
    /// The attempt counter. Starts at 1, so 0 never names an attempt.
    next: AtomicU32,
}

impl Connecting {
    pub(crate) fn new(connect: Arc<dyn ConnectApi>) -> Self {
        Self {
            connect,
            live: Mutex::new(HashMap::new()),
            next: AtomicU32::new(1),
        }
    }

    /// Starts an attempt and **returns immediately** with its id.
    ///
    /// Everything after this reaches the window as steps on `steps`: what is happening,
    /// what is being asked, and finally whether there is a session. The invoke that called
    /// this is already free.
    pub(crate) fn begin(&self, profile: ProfileId, steps: Arc<dyn ConnectSink>) -> AttemptId {
        let attempt = AttemptId(self.next.fetch_add(1, Ordering::SeqCst));
        let conversation = Arc::new(Conversation::new(attempt, steps));
        self.live
            .lock()
            .expect("attempt lock poisoned")
            .insert(attempt, Arc::clone(&conversation));

        let connect = Arc::clone(&self.connect);
        let questions = Arc::clone(&conversation) as Arc<dyn SshQuestions>;
        // **`spawn_blocking`, not `spawn`.** What runs here parks on a `std` channel
        // waiting for a person, and parking a runtime worker on a human is how a runtime
        // starves. This is the pool that exists for exactly that.
        tauri::async_runtime::spawn_blocking(move || {
            conversation.finished(connect.use_profile(&profile, &questions));
        });
        attempt
    }

    /// Delivers an answer to whichever attempt asked for it.
    ///
    /// **An id that names no live attempt is ignored rather than reported.** It is the
    /// ordinary consequence of a dialog the user abandoned, or of a window that reloaded
    /// while a connection was in flight — not something to say out loud, and certainly not
    /// something to guess a recipient for. A password delivered to the wrong question would
    /// be the worst possible version of being helpful.
    pub(crate) fn answer(&self, attempt: AttemptId, answer: ConnectAnswer) {
        let conversation = self
            .live
            .lock()
            .expect("attempt lock poisoned")
            .get(&attempt)
            .map(Arc::clone);
        if let Some(conversation) = conversation {
            conversation.answer(answer);
        }
    }

    /// Forgets an attempt that has ended, so the map does not grow for the life of the
    /// process.
    ///
    /// Called by the frontend when it has seen a terminal step, which is the only place
    /// that knows the conversation is over from the *window's* point of view — an attempt
    /// whose last question was never answered is still waiting until somebody says so.
    pub(crate) fn ended(&self, attempt: AttemptId) {
        self.live
            .lock()
            .expect("attempt lock poisoned")
            .remove(&attempt);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::Duration;

    use acter_core::{
        ConnectStep, Connectable, Connected, HostKeyQuestion, HostKeyState, SessionId,
    };

    use super::*;

    /// Long enough that a loaded machine is not what fails a test, short enough that a
    /// genuine deadlock is reported rather than hanging the suite.
    const PATIENCE: Duration = Duration::from_secs(5);

    /// A sink that hands each step to the test as it arrives.
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

    /// Connecting, faked: it either fails, succeeds, or asks a question first — which is
    /// the only three things that matter to this controller.
    struct Fake {
        asks: bool,
        outcome: Result<Connected, String>,
    }

    impl ConnectApi for Fake {
        fn connectable(&self) -> Vec<Connectable> {
            Vec::new()
        }

        fn use_profile(
            &self,
            _id: &ProfileId,
            questions: &Arc<dyn SshQuestions>,
        ) -> Result<Connected, String> {
            if self.asks {
                // Blocks until somebody answers, exactly as a real SSH connection does.
                questions.host_key(HostKeyQuestion {
                    host: "acter-ssh".to_owned(),
                    port: 2222,
                    fingerprint: "SHA256:offered".to_owned(),
                    state: HostKeyState::Unknown,
                    aside: None,
                });
            }
            self.outcome.clone()
        }

        fn connected(&self) -> Option<Connected> {
            None
        }
    }

    fn connecting(fake: Fake) -> Connecting {
        Connecting::new(Arc::new(fake) as Arc<dyn ConnectApi>)
    }

    fn scripted() -> ProfileId {
        ProfileId::Scripted {
            name: "builtin".to_owned(),
        }
    }

    /// **A far end that will not start ends the attempt with a sentence**, on the channel
    /// rather than as a rejected invoke — because the invoke was answered the moment the
    /// attempt began.
    #[test]
    fn a_profile_that_will_not_start_ends_the_attempt_with_a_speakable_sentence() {
        let (watcher, steps) = Watcher::new();
        let connecting = connecting(Fake {
            asks: false,
            outcome: Err("Acter could not reach acter-ssh on port 2222.".to_owned()),
        });

        connecting.begin(scripted(), watcher);

        let ConnectStep::Failed { why } = steps.recv_timeout(PATIENCE).expect("it ends") else {
            panic!("a far end that will not start fails");
        };
        assert!(why.ends_with('.'), "a spoken message ends: {why}");
        assert!(
            why.split_whitespace().count() >= 5,
            "a spoken message says what happened, not a label: {why}"
        );
    }

    /// And one that starts ends with the session it made, which is what the window attaches
    /// to.
    #[test]
    fn an_attempt_that_connects_ends_with_the_session_it_made() {
        let (watcher, steps) = Watcher::new();
        let connecting = connecting(Fake {
            asks: false,
            outcome: Ok(Connected {
                session: SessionId(4),
                label: "Scripted: builtin".to_owned(),
            }),
        });

        connecting.begin(scripted(), watcher);

        let ConnectStep::Arrived { connected } = steps.recv_timeout(PATIENCE).expect("it ends")
        else {
            panic!("an attempt that connected arrives");
        };
        assert_eq!(connected.session, SessionId(4));
    }

    /// **An answer reaches the attempt that asked, and no other.** Two attempts are alive
    /// at once whenever a user gives up on one dialog and starts again, and a password
    /// delivered to the wrong question is the worst version of being helpful.
    #[test]
    fn an_answer_reaches_only_the_attempt_that_asked_for_it() {
        let (watcher, steps) = Watcher::new();
        let connecting = connecting(Fake {
            asks: true,
            outcome: Ok(Connected {
                session: SessionId(1),
                label: "SSH".to_owned(),
            }),
        });

        let attempt = connecting.begin(scripted(), watcher);
        let asked = steps.recv_timeout(PATIENCE).expect("it asks");
        assert!(matches!(asked, ConnectStep::Asked { .. }));

        // An answer for a different attempt reaches nobody, so the question is still open.
        connecting.answer(AttemptId(attempt.0 + 99), ConnectAnswer::Trust);
        assert!(
            steps.recv_timeout(Duration::from_millis(200)).is_err(),
            "an answer for another attempt did not resolve this one"
        );

        connecting.answer(attempt, ConnectAnswer::Trust);
        assert!(matches!(
            steps.recv_timeout(PATIENCE).expect("it ends"),
            ConnectStep::Arrived { .. }
        ));
    }

    /// An attempt the window has finished with is forgotten, and answering it afterwards is
    /// a no-op rather than a panic — a second click on a button that already worked.
    #[test]
    fn an_attempt_that_ended_is_forgotten() {
        let (watcher, steps) = Watcher::new();
        let connecting = connecting(Fake {
            asks: false,
            outcome: Err("It did not start.".to_owned()),
        });

        let attempt = connecting.begin(scripted(), watcher);
        steps.recv_timeout(PATIENCE).expect("it ends");
        connecting.ended(attempt);
        connecting.answer(attempt, ConnectAnswer::Trust);
    }
}
