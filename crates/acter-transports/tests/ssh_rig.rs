//! Integration test: [`SshTransport`] against a real `sshd`, in the rig at `docker/ssh/`.
//!
//! Every test is ignored; bring the rig up as `docker/ssh/README.md` says, then run
//! `cargo test -p acter-transports --test ssh_rig -- --ignored --nocapture --test-threads=1`.
//! Several tests drive one container, hence one thread.

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::time::Instant as SystemInstant;

use acter_core::{
    Announcement, Clock, EventSink, ExitCode, HostKeyAnswer, HostKeyQuestion, HostKeyState,
    PacingConfig, PasswordQuestion, Secret, SessionApi, SessionEvent, SessionService, SshQuestions,
    Timer, Transport,
};
use acter_core::{HostKeyStore, RememberedHostKeys};
use acter_term::AlacrittyEngine;
use acter_transports::{KnownHosts, SshTarget, SshTransport};
use tokio::sync::mpsc::{Receiver, channel};
use tokio::sync::oneshot;
use tokio::time::{Instant, timeout};

/// Its login shell is dash, which never sets `$SHELL` itself, so a correct value came from sshd.
const DASH_USER: &str = "dashuser";

const ZSH_USER: &str = "zshuser";

const HOST: &str = "127.0.0.1";
const PORT: u16 = 2222;
const USER: &str = "acter";
/// Safe only because the rig is published on loopback.
const PASSWORD: &str = "acter";

const COLUMNS: u16 = 80;
const SCREEN_LINES: u16 = 24;

const PATIENCE: Duration = Duration::from_secs(20);

/// Must match what `SessionService` writes for Enter.
const ENTER: u8 = b'\r';

const READS: usize = 256;

struct Answers {
    key: Mutex<HostKeyAnswer>,
    passwords: Mutex<VecDeque<String>>,
    asked: Mutex<Vec<HostKeyQuestion>>,
    prompts: Mutex<Vec<PasswordQuestion>>,
    told: Mutex<Vec<String>>,
}

impl Answers {
    fn accepting() -> Arc<Self> {
        Self::with(HostKeyAnswer::Accept, [PASSWORD])
    }

    fn with<const N: usize>(key: HostKeyAnswer, passwords: [&str; N]) -> Arc<Self> {
        Arc::new(Self {
            key: Mutex::new(key),
            passwords: Mutex::new(passwords.iter().map(|each| (*each).to_owned()).collect()),
            asked: Mutex::new(Vec::new()),
            prompts: Mutex::new(Vec::new()),
            told: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<HostKeyQuestion> {
        self.asked.lock().unwrap().clone()
    }

    fn prompts(&self) -> Vec<PasswordQuestion> {
        self.prompts.lock().unwrap().clone()
    }

    fn told(&self) -> Vec<String> {
        self.told.lock().unwrap().clone()
    }
}

impl SshQuestions for Answers {
    fn host_key(&self, question: HostKeyQuestion) -> HostKeyAnswer {
        self.asked.lock().unwrap().push(question);
        *self.key.lock().unwrap()
    }

    fn password(&self, question: PasswordQuestion) -> Option<Secret> {
        self.prompts.lock().unwrap().push(question);
        self.passwords.lock().unwrap().pop_front().map(Secret::new)
    }

    fn tell(&self, sentence: &str) {
        self.told.lock().unwrap().push(sentence.to_owned());
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NONCE: AtomicUsize = AtomicUsize::new(0);
        let unique = NONCE.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("acter-ssh-rig-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("a scratch directory is made");
        Self(path)
    }

    fn empty(&self) -> Arc<KnownHosts> {
        Arc::new(KnownHosts::new(
            Arc::new(RememberedHostKeys::default()),
            None,
        ))
    }

    fn holding_another_key(&self) -> Arc<KnownHosts> {
        let recorded = RememberedHostKeys::default();
        recorded
            .accept(
                HOST,
                PORT,
                "ssh-ed25519",
                "SHA256:IzJE9oHP7rabiNsCSTceP2l1jW8/4WESW2jkk+JFiOU",
            )
            .expect("a fixture is written");
        Arc::new(KnownHosts::new(Arc::new(recorded), None))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Session {
    transport: SshTransport,
    reads: Receiver<Vec<u8>>,
    seen: String,
}

impl Session {
    async fn open(hosts: Arc<KnownHosts>, answers: Arc<Answers>) -> Result<Self, String> {
        Self::open_as(USER, hosts, answers, acter_transports::probe_patience()).await
    }

    async fn open_as(
        user: &str,
        hosts: Arc<KnownHosts>,
        answers: Arc<Answers>,
        patience: Duration,
    ) -> Result<Self, String> {
        let target = SshTarget {
            host: HOST.to_owned(),
            port: PORT,
            user: user.to_owned(),
        };
        let mut transport = SshTransport::connect(
            &target,
            hosts,
            answers as Arc<dyn SshQuestions>,
            COLUMNS,
            SCREEN_LINES,
            patience,
        )
        .await?;
        let (sender, reads) = channel(READS);
        transport.start(sender);
        Ok(Self {
            transport,
            reads,
            seen: String::new(),
        })
    }

    fn submit(&mut self, line: &str) {
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(ENTER);
        self.transport.write(&bytes).expect("the session is open");
    }

    async fn wait_for(&mut self, needle: &str) -> String {
        let deadline = Instant::now() + PATIENCE;
        while !self.seen.contains(needle) {
            let left = deadline.saturating_duration_since(Instant::now());
            let Ok(Some(read)) = timeout(left, self.reads.recv()).await else {
                panic!(
                    "waited {PATIENCE:?} for {needle:?} and the far end said: {:?}",
                    self.seen
                );
            };
            self.seen.push_str(&String::from_utf8_lossy(&read));
        }
        self.seen.clone()
    }

    async fn ended(&mut self) -> bool {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match timeout(left, self.reads.recv()).await {
                Ok(None) => return true,
                Ok(Some(read)) => self.seen.push_str(&String::from_utf8_lossy(&read)),
                Err(_) => return false,
            }
        }
    }
}

/// A remote pty echoes what is typed whether or not the line runs, so the word is split with
/// `''` and only the command actually running prints it whole.
fn spoken(word: &str) -> String {
    let (head, tail) = word.split_at(word.len() / 2);
    format!("echo {head}''{tail}")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn an_unknown_host_key_is_asked_about_and_then_remembered() {
    let scratch = Scratch::new();
    let hosts = scratch.empty();
    let answers = Answers::accepting();

    let mut session = Session::open(Arc::clone(&hosts), Arc::clone(&answers))
        .await
        .expect("the rig accepts the documented password");
    session.submit(&spoken("hello"));
    session.wait_for("hello").await;

    let asked = answers.asked();
    assert_eq!(asked.len(), 1, "asked exactly once");
    assert_eq!(asked[0].state, HostKeyState::Unknown);
    assert_eq!(asked[0].host, HOST);
    assert_eq!(asked[0].port, PORT);
    assert!(
        asked[0].fingerprint.starts_with("SHA256:"),
        "the fingerprint is the form ssh-keygen prints: {}",
        asked[0].fingerprint
    );
    assert_eq!(asked[0].aside, None, "there was nothing wrong to mention");

    let second = Answers::accepting();
    Session::open(hosts, Arc::clone(&second))
        .await
        .expect("a host that was accepted connects again");
    assert!(
        second.asked().is_empty(),
        "a key that was written down is never asked about again"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_refused_host_key_is_reported_as_the_users_own_decision() {
    let scratch = Scratch::new();
    let answers = Answers::with(HostKeyAnswer::Refuse, [PASSWORD]);

    let Err(why) = Session::open(scratch.empty(), Arc::clone(&answers)).await else {
        panic!("a refused key is not a session");
    };

    assert!(
        why.contains("did not accept") && why.contains("host key"),
        "it says what the user decided: {why}"
    );
    assert!(why.ends_with('.'), "it is a whole sentence: {why}");
    assert!(
        answers.prompts().is_empty(),
        "a refused key is refused before anybody is asked for a password"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_changed_host_key_is_a_different_question() {
    let scratch = Scratch::new();
    let answers = Answers::with(HostKeyAnswer::Refuse, [PASSWORD]);

    let Err(why) = Session::open(scratch.holding_another_key(), Arc::clone(&answers)).await else {
        panic!("a refused key is not a session");
    };

    let asked = answers.asked();
    assert_eq!(asked.len(), 1);
    let HostKeyState::Changed { recorded } = &asked[0].state else {
        panic!("a server whose key is not the recorded one has changed: {asked:?}");
    };
    assert_ne!(
        recorded, &asked[0].fingerprint,
        "both fingerprints travel, so the two can be compared aloud"
    );
    assert!(
        why.contains("changed"),
        "and refusing says the alarming thing rather than the routine one: {why}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_wrong_password_is_asked_for_again_and_says_so() {
    let scratch = Scratch::new();
    let answers = Answers::with(HostKeyAnswer::Accept, ["not-the-password", PASSWORD]);

    Session::open(scratch.empty(), Arc::clone(&answers))
        .await
        .expect("the second password is the right one");

    let prompts = answers.prompts();
    assert_eq!(prompts.len(), 2, "asked twice");
    assert!(!prompts[0].again, "the first time is not a retry");
    assert!(prompts[1].again, "the second time says so");
    assert_eq!(prompts[0].host, HOST);
    assert_eq!(prompts[0].user, USER);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn giving_no_password_ends_the_attempt_with_a_sentence() {
    let scratch = Scratch::new();
    let answers = Answers::with(HostKeyAnswer::Accept, []);

    let Err(why) = Session::open(scratch.empty(), Arc::clone(&answers)).await else {
        panic!("no password is no session");
    };

    assert!(why.contains("no password was given"), "{why}");
    assert!(why.ends_with('.'), "it is a whole sentence: {why}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn what_the_far_end_writes_reaches_the_transport() {
    let scratch = Scratch::new();
    let mut session = Session::open(scratch.empty(), Answers::accepting())
        .await
        .expect("the rig connects");

    session.submit(&format!("{}; {} >&2", spoken("out"), spoken("err")));

    let seen = session.wait_for("err").await;
    assert!(seen.contains("out"), "standard output arrived: {seen:?}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn an_interrupt_stops_a_command_that_is_running() {
    let scratch = Scratch::new();
    let mut session = Session::open(scratch.empty(), Answers::accepting())
        .await
        .expect("the rig connects");
    session.submit(&spoken("ready"));
    session.wait_for("ready").await;

    session.submit("sleep 60");
    // Interrupting a command the far end has not started yet proves nothing.
    tokio::time::sleep(Duration::from_millis(500)).await;
    session.transport.interrupt().expect("the session is open");

    session.submit(&spoken("stopped"));
    session.wait_for("stopped").await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_resize_reaches_the_far_end() {
    let scratch = Scratch::new();
    let mut session = Session::open(scratch.empty(), Answers::accepting())
        .await
        .expect("the rig connects");
    session.submit(&spoken("ready"));
    session.wait_for("ready").await;

    session
        .transport
        .resize(100, 40)
        .expect("the session is open");
    // `stty size` prints rows, then columns.
    session.submit("stty size");

    session.wait_for("40 100").await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn the_session_ends_when_the_shell_exits() {
    let scratch = Scratch::new();
    let mut session = Session::open(scratch.empty(), Answers::accepting())
        .await
        .expect("the rig connects");
    session.submit(&spoken("ready"));
    session.wait_for("ready").await;

    session.submit("exit");

    assert!(
        session.ended().await,
        "a shell that exited closes the read channel: {:?}",
        session.seen
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn connecting_says_what_it_is_doing() {
    let scratch = Scratch::new();
    let answers = Answers::accepting();

    Session::open(scratch.empty(), Arc::clone(&answers))
        .await
        .expect("the rig connects");

    let told = answers.told();
    assert!(
        told.iter().any(|said| said.contains("Connecting")),
        "it says it is connecting: {told:?}"
    );
    assert!(
        told.iter().all(|said| said.ends_with('.')),
        "every one of them is a whole sentence: {told:?}"
    );
    assert!(
        !told.iter().any(|said| said.contains(PASSWORD)),
        "and none of them carries the password: {told:?}"
    );
}

/// Prints which mechanism alone stopped a remote command, and asserts only that one did.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn what_actually_interrupts_a_remote_command() {
    let by_request = stopped_by(Mechanism::SignalRequest).await;
    let by_byte = stopped_by(Mechanism::ControlByte).await;

    println!("--- what interrupts a remote command, measured against docker/ssh ---");
    println!(
        "  a `signal` channel request alone: {}",
        verdict(by_request)
    );
    println!("  the byte 0x03 alone:              {}", verdict(by_byte));

    assert!(
        by_request || by_byte,
        "neither mechanism stopped a remote command, so this measurement is stale"
    );
}

fn verdict(stopped: bool) -> &'static str {
    if stopped {
        "stopped the command"
    } else {
        "did nothing"
    }
}

enum Mechanism {
    SignalRequest,
    ControlByte,
}

async fn stopped_by(mechanism: Mechanism) -> bool {
    use russh::client;

    struct Trusting;
    impl client::Handler for Trusting {
        type Error = russh::Error;
        /// Trusts any key: only for the rig on loopback.
        async fn check_server_key(
            &mut self,
            _offered: &russh::keys::PublicKeyOrCertificate,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    let mut connection =
        client::connect(Arc::new(client::Config::default()), (HOST, PORT), Trusting)
            .await
            .expect("the rig is up");
    assert!(
        connection
            .authenticate_password(USER, PASSWORD)
            .await
            .expect("the rig answers")
            .success(),
        "the rig accepts the documented password"
    );
    let mut channel = connection
        .channel_open_session()
        .await
        .expect("a session channel opens");
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await
        .expect("the rig gives a terminal");
    channel.request_shell(true).await.expect("a shell starts");

    // `sleep` is written only once the shell has drawn something, or nothing reads it.
    read_for(&mut channel, Duration::from_secs(2)).await;
    channel
        .data_bytes(b"sleep 60\r".to_vec())
        .await
        .expect("the line is written");
    tokio::time::sleep(Duration::from_millis(500)).await;

    match mechanism {
        Mechanism::SignalRequest => channel
            .signal(russh::Sig::INT)
            .await
            .expect("the request is sent"),
        Mechanism::ControlByte => channel
            .data_bytes(vec![0x03])
            .await
            .expect("the byte is written"),
    }

    channel
        .data_bytes(b"echo stopped\r".to_vec())
        .await
        .expect("the line is written");
    read_for(&mut channel, Duration::from_secs(3))
        .await
        .contains("stopped")
}

async fn read_for(channel: &mut russh::Channel<russh::client::Msg>, patience: Duration) -> String {
    let deadline = Instant::now() + patience;
    let mut seen = String::new();
    while let Ok(Some(message)) = timeout(
        deadline.saturating_duration_since(Instant::now()),
        channel.wait(),
    )
    .await
    {
        if let russh::ChannelMsg::Data { data } = message {
            seen.push_str(&String::from_utf8_lossy(&data));
        }
    }
    seen
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn the_far_end_says_what_shell_it_is() {
    let scratch = Scratch::new();
    let session = Session::open(scratch.empty(), Answers::accepting())
        .await
        .expect("the rig connects");

    let far_end = session.transport.far_end();

    assert_eq!(far_end.name().as_deref(), Some("bash"));
    assert_eq!(far_end.shell.as_deref(), Some("/bin/bash"));
    assert_eq!(
        far_end.flavour.as_deref(),
        Some("bash"),
        "bash sets a version variable, which is the most certain evidence there is"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn an_account_whose_shell_is_not_bash_is_still_named_correctly() {
    let scratch = Scratch::new();
    let session = Session::open_as(
        DASH_USER,
        scratch.empty(),
        Answers::accepting(),
        acter_transports::probe_patience(),
    )
    .await
    .expect("the rig connects as the dash account");

    let far_end = session.transport.far_end();

    assert_eq!(far_end.shell.as_deref(), Some("/bin/dash"));
    assert_eq!(
        far_end.flavour, None,
        "dash sets no version variable, so nothing could have been invented"
    );
    assert_eq!(far_end.name().as_deref(), Some("dash"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_probe_that_runs_out_of_time_still_gives_a_working_session() {
    let scratch = Scratch::new();
    let mut session = Session::open_as(USER, scratch.empty(), Answers::accepting(), Duration::ZERO)
        .await
        .expect("a probe that answered nothing is not a failure to connect");

    assert_eq!(
        session.transport.far_end().name(),
        None,
        "nothing is claimed about a far end that did not answer in time"
    );
    session.submit(&spoken("working"));
    session.wait_for("working").await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn the_probe_is_never_heard_by_the_session() {
    let scratch = Scratch::new();
    let mut session = Session::open(scratch.empty(), Answers::accepting())
        .await
        .expect("the rig connects");
    assert!(
        session.transport.far_end().name().is_some(),
        "the probe did answer, so this test is about where the answer went"
    );

    session.submit(&spoken("afterwards"));
    let seen = session.wait_for("afterwards").await;

    assert!(
        !seen.contains("ACTER SHELL="),
        "the probe's own output never reached the session: {seen:?}"
    );
    assert!(
        !seen.contains("BASH_VERSION") && !seen.contains("printf"),
        "and neither did the question: {seen:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn the_byte_that_ends_a_bash_session_over_ssh() {
    let scratch = Scratch::new();
    let mut session = Session::open(scratch.empty(), Answers::accepting())
        .await
        .expect("the rig connects");
    session.submit(&spoken("ready"));
    session.wait_for("ready").await;

    session
        .transport
        .write(&[0x04])
        .expect("the session is open");

    assert!(
        session.ended().await,
        "0x04 at an empty bash prompt ends the session: {:?}",
        session.seen
    );
}

/// Measured against `docker/ssh`'s `zshuser` (zsh 5.9, OpenSSH 9.2): `0x04` at an empty zsh
/// prompt ends the session.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn the_byte_that_ends_a_zsh_session_over_ssh() {
    let scratch = Scratch::new();
    let mut session = Session::open_as(
        ZSH_USER,
        scratch.empty(),
        Answers::accepting(),
        acter_transports::probe_patience(),
    )
    .await
    .expect("the rig connects as the zsh account");
    session.submit(&spoken("ready"));
    session.wait_for("ready").await;

    session
        .transport
        .write(&[0x04])
        .expect("the session is open");

    assert!(
        session.ended().await,
        "0x04 at an empty zsh prompt ends the session: {:?}",
        session.seen
    );
}

struct RealClock(SystemInstant);

impl Clock for RealClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }

    fn timer(&self, after: Duration) -> Timer {
        let (fire, fired) = oneshot::channel();
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            let _ = fire.send(());
        });
        Timer::new(fired)
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<SessionEvent>>);

impl EventSink for Recorder {
    fn send(&self, event: SessionEvent) {
        self.0.lock().unwrap().push(event);
    }
}

impl Recorder {
    fn events(&self) -> Vec<SessionEvent> {
        self.0.lock().unwrap().clone()
    }
}

struct Pipeline {
    session: SessionService,
    recorder: Arc<Recorder>,
}

impl Pipeline {
    async fn connect(scratch: &Scratch) -> Self {
        Self::connect_as(USER, scratch).await
    }

    async fn connect_as(user: &str, scratch: &Scratch) -> Self {
        let target = SshTarget {
            host: HOST.to_owned(),
            port: PORT,
            user: user.to_owned(),
        };
        let transport = SshTransport::connect(
            &target,
            scratch.empty(),
            Answers::accepting() as Arc<dyn SshQuestions>,
            COLUMNS,
            SCREEN_LINES,
            acter_transports::probe_patience(),
        )
        .await
        .expect("the rig connects");

        let name = transport.far_end().name();
        let facts = acter_shells::over_ssh(name.as_deref());
        let session = SessionService::start(
            Box::new(transport),
            Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
            Arc::new(RealClock(SystemInstant::now())) as Arc<dyn Clock>,
            PacingConfig::default(),
            facts,
        );

        let recorder = Arc::new(Recorder::default());
        session.attach_session(
            acter_core::SessionId(1),
            Arc::clone(&recorder) as Arc<dyn EventSink>,
        );
        Self { session, recorder }
    }

    async fn wait_until(&self, what: &str, mut ready: impl FnMut(&[SessionEvent]) -> bool) {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if ready(&self.recorder.events()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.print("what the session said before it was given up on");
        panic!("waited {PATIENCE:?} for {what}");
    }

    fn print(&self, title: &str) {
        let seen = self.recorder.events();
        println!("--- {title} ---");
        for event in &seen {
            println!("  {event:?}");
        }
        println!("--- {} events in total ---", seen.len());
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn what_a_session_says_when_it_has_just_connected() {
    let scratch = Scratch::new();
    let pipeline = Pipeline::connect(&scratch).await;

    // Six seconds covers the prompt, the setup and the marked prompt redrawn behind it.
    tokio::time::sleep(Duration::from_secs(6)).await;
    pipeline.print("what a freshly connected SSH session said, in order");

    let seen = pipeline.recorder.events();
    let spoken: String = seen
        .iter()
        .filter_map(|event| match event {
            SessionEvent::Output { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        spoken.contains('$'),
        "the prompt the far end had already drawn never reached the frontend: {seen:?}"
    );
    assert!(
        seen.iter().any(|event| matches!(
            event,
            SessionEvent::Announce {
                announcement: Announcement::ReadAloud { text },
                ..
            } if text.contains('$')
        )),
        "and a listener hears it rather than having to go looking: {seen:?}"
    );
}

/// OSC 133 markers cross SSH whole, measured against `docker/ssh` (Debian bookworm, bash
/// 5.2.15, OpenSSH 9.2): the prompt arrives as an `A`..`B` pair and a `D;7` survives the trip.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn the_markers_a_session_sets_itself_up_with_cross_an_ssh_connection() {
    let scratch = Scratch::new();
    let pipeline = Pipeline::connect(&scratch).await;

    pipeline
        .wait_until("the far end to draw a marked prompt", |seen| {
            seen.iter()
                .any(|event| matches!(event, SessionEvent::PromptDrawn { .. }))
        })
        .await;

    pipeline
        .session
        .submit_command(acter_core::SessionId(1), "(exit 7)");
    pipeline
        .wait_until("a verdict for a command that failed", |seen| {
            seen.iter().any(|event| {
                matches!(
                    event,
                    SessionEvent::Announce {
                        announcement: Announcement::Failed {
                            exit_code: ExitCode(7)
                        },
                        ..
                    }
                )
            })
        })
        .await;
    pipeline.print("what an SSH session said once it had set itself up");

    let seen = pipeline.recorder.events();
    assert!(
        !seen.contains(&SessionEvent::IntegrationUnavailable),
        "a session that marks its boundaries never says it has none: {seen:?}"
    );

    // `sshd` prints `Last login:` before the prompt, so the setup goes out on the banner's line.
    let spoken: String = seen
        .iter()
        .filter_map(|event| match event {
            SessionEvent::Announce {
                announcement: Announcement::ReadAloud { text },
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !spoken.contains("__acter_prompt"),
        "Acter's own setup command was read to the listener: {spoken:?}"
    );

    // The echoed setup lands in the block's heading when it falls in `B..C`, and in its
    // output when a banner or a redraw puts it elsewhere.
    let headed = seen.iter().any(|event| {
        matches!(
            event,
            SessionEvent::CommandStarted {
                command_line: Some(line),
                ..
            } if line.contains("__acter_prompt")
        )
    });
    let rendered: String = seen
        .iter()
        .filter_map(|event| match event {
            SessionEvent::Output { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        headed || rendered.contains("__acter_prompt"),
        "the disclosure has to be readable back, and it is in neither the heading nor the          buffer: {seen:?}"
    );
}

/// Measured against `docker/ssh`'s `zshuser` (Debian bookworm, zsh 5.9, OpenSSH 9.2): the
/// prompt arrives marked and a `D;7` from zsh's own prompt expansion survives the trip.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_remote_zsh_sets_itself_up_and_says_how_a_command_went() {
    let scratch = Scratch::new();
    let pipeline = Pipeline::connect_as(ZSH_USER, &scratch).await;

    pipeline
        .wait_until("the far end to draw a marked prompt", |seen| {
            seen.iter()
                .any(|event| matches!(event, SessionEvent::PromptDrawn { .. }))
        })
        .await;

    pipeline
        .session
        .submit_command(acter_core::SessionId(1), "(exit 7)");
    pipeline
        .wait_until("a verdict for a command that failed", |seen| {
            seen.iter().any(|event| {
                matches!(
                    event,
                    SessionEvent::Announce {
                        announcement: Announcement::Failed {
                            exit_code: ExitCode(7)
                        },
                        ..
                    }
                )
            })
        })
        .await;
    pipeline.print("what a remote zsh said once it had set itself up");

    let seen = pipeline.recorder.events();
    assert!(
        !seen.contains(&SessionEvent::IntegrationUnavailable),
        "a session that marks its boundaries never says it has none: {seen:?}"
    );

    let spoken: String = seen
        .iter()
        .filter_map(|event| match event {
            SessionEvent::Announce {
                announcement: Announcement::ReadAloud { text },
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !spoken.contains("__acter_prompt"),
        "Acter's own setup command was read to the listener: {spoken:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn an_account_whose_shell_is_zsh_is_named_as_zsh() {
    let scratch = Scratch::new();
    let session = Session::open_as(
        ZSH_USER,
        scratch.empty(),
        Answers::accepting(),
        acter_transports::probe_patience(),
    )
    .await
    .expect("the rig connects as the zsh account");

    let far_end = session.transport.far_end();

    assert_eq!(far_end.shell.as_deref(), Some("/bin/zsh"));
    assert_eq!(far_end.name().as_deref(), Some("zsh"));
    assert_eq!(
        far_end.flavour.as_deref(),
        Some("zsh"),
        "zsh sets a version variable of its own, which is the certain evidence"
    );
}
