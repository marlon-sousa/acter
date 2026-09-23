//! Controller: `Connecting`, one attempt to connect from the invoke that starts it to the
//! answer that lets it finish.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use acter_core::{
    AttemptId, ConnectAnswer, ConnectApi, ConnectQuestions, ConnectSink, Conversation, ProfileId,
    SetUp,
};

pub(crate) struct Connecting {
    connect: Arc<dyn ConnectApi>,
    live: Mutex<HashMap<AttemptId, Arc<Conversation>>>,
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

    /// Returns at once: a synchronous `#[tauri::command]` runs on the main thread, and holding
    /// it while the attempt waits for a person would block the invoke carrying the answer.
    pub(crate) fn begin(
        &self,
        profile: ProfileId,
        set_up: SetUp,
        origin: Option<String>,
        steps: Arc<dyn ConnectSink>,
    ) -> AttemptId {
        let attempt = AttemptId(self.next.fetch_add(1, Ordering::SeqCst));
        let conversation = Arc::new(Conversation::new(attempt, steps));
        self.live
            .lock()
            .expect("attempt lock poisoned")
            .insert(attempt, Arc::clone(&conversation));

        let connect = Arc::clone(&self.connect);
        let questions = Arc::clone(&conversation) as Arc<dyn ConnectQuestions>;
        // Blocking pool: this parks on a `std` channel waiting for a person, which would starve
        // a runtime worker.
        tauri::async_runtime::spawn_blocking(move || {
            conversation.finished(connect.use_profile(
                &profile,
                set_up,
                origin.as_deref(),
                &questions,
            ));
        });
        attempt
    }

    /// An id that names no live attempt is ignored.
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
            _set_up: SetUp,
            _origin: Option<&str>,
            questions: &Arc<dyn ConnectQuestions>,
        ) -> Result<Connected, String> {
            if self.asks {
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

        fn saved(&self) -> acter_core::SavedConnections {
            acter_core::SavedConnections {
                rows: Vec::new(),
                unreadable: None,
            }
        }

        fn save_connection(&self, _name: &str) -> Result<String, String> {
            unreachable!("this controller never saves")
        }

        fn rename_connection(&self, _from: &str, _to: &str) -> Result<String, String> {
            unreachable!("this controller never renames")
        }

        fn forget_connection(&self, _name: &str) -> Result<String, String> {
            unreachable!("this controller never forgets")
        }

        fn offer_to_save(&self) -> bool {
            true
        }

        fn stop_offering_to_save(&self) -> Result<(), String> {
            unreachable!("this controller records no preference")
        }

        fn requested_at_launch(&self) -> Option<acter_core::LaunchRequest> {
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

    #[test]
    fn a_profile_that_will_not_start_ends_the_attempt_with_a_speakable_sentence() {
        let (watcher, steps) = Watcher::new();
        let connecting = connecting(Fake {
            asks: false,
            outcome: Err("Acter could not reach acter-ssh on port 2222.".to_owned()),
        });

        connecting.begin(scripted(), SetUp::Yes, None, watcher);

        let ConnectStep::Failed { why } = steps.recv_timeout(PATIENCE).expect("it ends") else {
            panic!("a far end that will not start fails");
        };
        assert!(why.ends_with('.'), "a spoken message ends: {why}");
        assert!(
            why.split_whitespace().count() >= 5,
            "a spoken message says what happened, not a label: {why}"
        );
    }

    #[test]
    fn an_attempt_that_connects_ends_with_the_session_it_made() {
        let (watcher, steps) = Watcher::new();
        let connecting = connecting(Fake {
            asks: false,
            outcome: Ok(Connected {
                session: SessionId(4),
                label: "Scripted: builtin".to_owned(),
                note: None,
                limit_explained: false,
                saved_as: None,
                line_owner: acter_core::LineOwner::FarEnd,
            }),
        });

        connecting.begin(scripted(), SetUp::Yes, None, watcher);

        let ConnectStep::Arrived { connected } = steps.recv_timeout(PATIENCE).expect("it ends")
        else {
            panic!("an attempt that connected arrives");
        };
        assert_eq!(connected.session, SessionId(4));
    }

    #[test]
    fn an_answer_reaches_only_the_attempt_that_asked_for_it() {
        let (watcher, steps) = Watcher::new();
        let connecting = connecting(Fake {
            asks: true,
            outcome: Ok(Connected {
                session: SessionId(1),
                label: "SSH".to_owned(),
                note: None,
                limit_explained: false,
                saved_as: None,
                line_owner: acter_core::LineOwner::FarEnd,
            }),
        });

        let attempt = connecting.begin(scripted(), SetUp::Yes, None, watcher);
        let asked = steps.recv_timeout(PATIENCE).expect("it asks");
        assert!(matches!(asked, ConnectStep::Asked { .. }));

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

    #[test]
    fn an_attempt_that_ended_is_forgotten() {
        let (watcher, steps) = Watcher::new();
        let connecting = connecting(Fake {
            asks: false,
            outcome: Err("It did not start.".to_owned()),
        });

        let attempt = connecting.begin(scripted(), SetUp::Yes, None, watcher);
        steps.recv_timeout(PATIENCE).expect("it ends");
        connecting.ended(attempt);
        connecting.answer(attempt, ConnectAnswer::Trust);
    }
}
