//! Integration test: [`SshTransport`] against a real `sshd`, in the rig at `docker/ssh/`.
//!
//! **Every test here is `#[ignore]`d**, so `cargo test --workspace` opens no socket and
//! depends on nothing being installed. Unlike the local-shell suites, CI does *not* run
//! these: the Windows runner cannot run a Debian container, and a rig faked on Windows
//! would be a different server from the one this entry was measured against — which is
//! B4.5's lesson exactly. They are a developer's, run against the documented container.
//!
//! Bring the far end up as `docker/ssh/README.md` says, then:
//! `cargo test -p acter-transports --test ssh_rig -- --ignored --nocapture --test-threads=1`
//!
//! `--test-threads=1` because several of these drive one container, and `--nocapture`
//! because [`what_actually_interrupts_a_remote_command`] is a *measurement* rather than an
//! assertion and its printed output is the point.
//!
//! **Each test gets its own record of host keys**, in a directory of its own, so "this host
//! is unknown" and "this host is already trusted" are both reachable without touching
//! anything on the developer's machine — and so no test can pass because a previous one
//! left a file behind.

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
// `tokio::time::Instant` is the one this file waits on; the pipeline's clock is the real
// one, so both are in scope and each is named for what it is.
use std::time::Instant as SystemInstant;

use acter_core::{
    Announcement, Clock, EventSink, ExitCode, HostKeyAnswer, HostKeyQuestion, HostKeyState,
    PacingConfig, PasswordQuestion, Secret, SessionApi, SessionEvent, SessionService, SshQuestions,
    Timer, Transport,
};
use acter_term::AlacrittyEngine;
use acter_transports::{KnownHosts, SshTarget, SshTransport};
use tokio::sync::mpsc::{Receiver, channel};
use tokio::sync::oneshot;
use tokio::time::{Instant, timeout};

/// The account whose login shell is `dash`, which is the control for the probe: dash has
/// no behaviour of setting `$SHELL` itself, so a correct value arriving for it can only
/// have come from sshd (see the Dockerfile).
const DASH_USER: &str = "dashuser";

/// The rig, as `docker/ssh/README.md` runs it: loopback only, so nothing on any network
/// this machine joins can reach it.
const HOST: &str = "127.0.0.1";
const PORT: u16 = 2222;
const USER: &str = "acter";
/// Weak on purpose and safe only because of how the container is published. It is never a
/// template for anything shipped.
const PASSWORD: &str = "acter";

/// The emulated screen the container is told about, matching the composition root's.
const COLUMNS: u16 = 80;
const SCREEN_LINES: u16 = 24;

/// Long enough that a slow machine is not what fails a test, short enough that a genuine
/// hang is reported as one rather than sitting there.
const PATIENCE: Duration = Duration::from_secs(20);

/// What `SessionService` writes when the user presses Enter, restated here so this suite
/// submits exactly what the product submits.
const ENTER: u8 = b'\r';

/// How many reads the session channel holds, matching `SessionService`'s own.
const READS: usize = 256;

// --- The fake that answers what the connection asks ------------------------------------

/// Everything a person would be asked, answered from a script and recorded.
///
/// **This is the whole reason the questions are a port** (spec B9): the same transport that
/// puts a modal dialog in front of a screen reader user is driven here by a struct with a
/// queue in it, and it cannot tell the difference.
struct Answers {
    key: Mutex<HostKeyAnswer>,
    passwords: Mutex<VecDeque<String>>,
    asked: Mutex<Vec<HostKeyQuestion>>,
    prompts: Mutex<Vec<PasswordQuestion>>,
    told: Mutex<Vec<String>>,
}

impl Answers {
    /// Accepts whatever host key it is shown and gives the right password once.
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

    /// The host-key questions that were actually put, which is what "asked about" means.
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

// --- A record of host keys nothing else in this run shares ------------------------------

/// A directory of this test's own, removed when it goes.
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

    /// A record of host keys with nothing in it: every host is unknown.
    fn empty(&self) -> Arc<KnownHosts> {
        Arc::new(KnownHosts::new(self.0.join("acter_known_hosts"), None))
    }

    /// A record holding some *other* key for the rig, which is what makes the server's real
    /// key a **changed** one — the security case, reached without rebuilding the container.
    fn holding_another_key(&self) -> Arc<KnownHosts> {
        let path = self.0.join("acter_known_hosts");
        fs::write(
            &path,
            format!(
                "[{HOST}]:{PORT} ssh-ed25519 \
                 AAAAC3NzaC1lZDI1NTE5AAAAIHKQ43TBPmSEIjzocj1VrRSKA4Vxa65wu0uNWQx49Tfk\n"
            ),
        )
        .expect("a fixture is written");
        Arc::new(KnownHosts::new(path, None))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// --- A started session --------------------------------------------------------------

/// A connected, started transport and the reads it has produced.
struct Session {
    transport: SshTransport,
    reads: Receiver<Vec<u8>>,
    seen: String,
}

impl Session {
    async fn open(hosts: Arc<KnownHosts>, answers: Arc<Answers>) -> Result<Self, String> {
        Self::open_as(USER, hosts, answers, acter_transports::probe_patience()).await
    }

    /// The same, for the two tests that need another account or another deadline.
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

    /// Submits a line the way the session service does.
    fn submit(&mut self, line: &str) {
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(ENTER);
        self.transport.write(&bytes).expect("the session is open");
    }

    /// Reads until `needle` has been seen, or gives up loudly.
    ///
    /// Accumulating rather than matching one read is not optional: a marker split across
    /// two reads is the case this whole architecture is built around, and over a network it
    /// is the normal case rather than the awkward one.
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

    /// Whether the far end ended the session, within the patience.
    async fn ended(&mut self) -> bool {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match timeout(left, self.reads.recv()).await {
                // The channel closing is how a transport reports that the far end went,
                // which is the port's own rule and not an error variant.
                Ok(None) => return true,
                Ok(Some(read)) => self.seen.push_str(&String::from_utf8_lossy(&read)),
                Err(_) => return false,
            }
        }
    }
}

/// A command whose *output* is `word` and whose typed text is not.
///
/// **Without this, every assertion in this file was satisfied by the echo.** A remote pty
/// echoes what is typed into it whether or not anything ever runs the line — so waiting for
/// "stopped" after submitting `echo stopped` succeeds even when the far end is still busy
/// with the command that was supposed to have been interrupted. Splitting the word with an
/// empty quoted string means bash prints `stopped` while the terminal echoed `stop''ped`,
/// and only the command actually running can produce the needle.
fn spoken(word: &str) -> String {
    let (head, tail) = word.split_at(word.len() / 2);
    format!("echo {head}''{tail}")
}

// --- Host keys -----------------------------------------------------------------------

/// The routine first connection, which is the one every user meets: Acter asks, with a
/// fingerprint they can compare, and having been told once does not ask again.
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

    // The second connection, against the same record: silent.
    let second = Answers::accepting();
    Session::open(hosts, Arc::clone(&second))
        .await
        .expect("a host that was accepted connects again");
    assert!(
        second.asked().is_empty(),
        "a key that was written down is never asked about again"
    );
}

/// Refusing is the default and it has to *work*: no session, and a sentence about the
/// decision the user made rather than about a socket.
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

/// **The security case.** The same three-state check, reached by having a different key on
/// file for this host — which is what a rebuilt server, or somebody sitting in the middle,
/// produces. The genuine article (`ACTER_SSH_REKEY=1`) is what the accessibility pass uses;
/// this is what keeps the code path honest between passes.
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

// --- Signing in ------------------------------------------------------------------------

/// A wrong password is asked for again, and the second question *says* it is a second
/// question — a dialog that reappeared with no explanation would be indistinguishable from
/// one that had not been submitted.
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

/// Declining to give a password ends the attempt as a decision rather than a failure, and
/// says so in a sentence.
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

// --- The session itself ----------------------------------------------------------------

/// The far end's output reaches the transport, including what it writes to standard error
/// — a session that rendered a command's output and dropped its error messages would be
/// lying to somebody who cannot see the screen.
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

/// **The interrupt crosses the connection.** Asserted by what happens *after* it: a
/// command that would have run for a minute is over, and the next one answers at once.
/// Nothing here asserts that a signal was delivered — whether a command actually ended is
/// observed the ordinary way, in the bytes that follow (the port's own rule).
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
    // Long enough that the far end has certainly started it: interrupting a command that
    // has not begun proves nothing about interrupting one that has.
    tokio::time::sleep(Duration::from_millis(500)).await;
    session.transport.interrupt().expect("the session is open");

    session.submit(&spoken("stopped"));
    session.wait_for("stopped").await;
}

/// A window change reaches the far end, which is what makes a resize more than a local
/// redraw: the remote program has to be told, or it wraps its output for a screen that is
/// not there.
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
    // `stty size` prints rows then columns, which is the far end's own account of the
    // window rather than ours.
    session.submit("stty size");

    session.wait_for("40 100").await;
}

/// A shell that exits ends the session, and the way it is reported is the channel closing
/// — which is what everything above a transport watches for.
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

/// Connecting says what it is doing while it does it, because a listener with no feedback
/// cannot tell a slow network from a dead one (spec B9, decision 6).
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

// --- A measurement, not an assertion ---------------------------------------------------

/// **What actually stops a command at the far end**, measured rather than assumed.
///
/// Spec B9 was drafted saying that over SSH an interrupt "is a channel request that travels
/// outside the data stream", which is what the protocol offers and what
/// [`Transport::interrupt`]'s own documentation says the method exists for. Whether
/// OpenSSH's `sshd` acts on a `signal` request for an interactive session is a different
/// question, and B4.5's lesson is that "should" is not evidence.
///
/// So this sends each mechanism *alone*, on a connection of its own, and reports whether
/// the command that was running stopped. The transport sends both, cheapest first; this is
/// what says whether that is belt and braces or the only thing holding it up.
///
/// A measurement, so it asserts nothing about which one wins — it prints, and
/// `--nocapture` is why. It does assert that *something* works, because a run where neither
/// did would mean the far end changed under us and the finding below is stale.
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

/// Runs `sleep 60` on a connection of its own, applies one mechanism, and reports whether
/// the shell came back to a prompt inside a few seconds.
async fn stopped_by(mechanism: Mechanism) -> bool {
    use russh::client;

    struct Trusting;
    impl client::Handler for Trusting {
        type Error = russh::Error;
        /// The rig, on loopback, in a measurement that establishes nothing about identity.
        /// Every path a *user* reaches goes through `KnownHosts` and a question.
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

    // Wait until the shell has drawn something, so `sleep` is submitted to a shell that is
    // ready for it rather than into a channel nobody is reading yet.
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

/// Everything the channel says for this long, as text.
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

// --- What the far end is, asked on a channel of its own -------------------------------

/// The probe answers, and it answers with the shell's own account of itself.
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

/// **The control for decision 7.** `dash` never sets `$SHELL` for itself, so a correct
/// answer for this account can only have come from sshd's own reading of the passwd entry —
/// which is what makes the probe trustworthy for the accounts it exists to serve rather
/// than only for the one that would have flattered it.
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

/// **Never a gate.** A deadline that has already passed costs the answer and nothing else:
/// the session opens, works, and simply has no name for its far end.
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

/// **The answer never reaches the terminal buffer**, which is why it is a channel of its
/// own rather than a line typed into the session — a command nobody typed, read aloud, is
/// B4.9's whole subject.
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

/// **What ends a bash session over this transport**, measured rather than assumed.
///
/// Spec B9, decision 7 says "bash ends on `0x04`" as though it were settled. It was not:
/// `acter-shells`' own bash adapter answers `None` for end-of-input on purpose, because the
/// obvious control byte is exactly what B5.2 measured and *disproved* for PowerShell, where
/// neither candidate ends a session and both are echoed as caret text. So the byte is sent
/// here and the far end is watched.
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

// --- What a freshly connected session says, measured ------------------------------------

/// A real clock, for the two tests that build the whole pipeline rather than watching bytes.
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

/// Everything the frontend would have received.
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

/// The whole pipeline a window has, over a real SSH connection.
///
/// **Exactly what the composition root builds for an SSH far end**, so what these tests
/// assert is what a listener gets rather than what bytes arrived: the transport, a real
/// terminal engine, the real pacing policy, and `over_ssh`'s shell facts — which since B9.5
/// carry a setup, and are therefore the thing under measurement here.
struct Pipeline {
    session: SessionService,
    recorder: Arc<Recorder>,
}

impl Pipeline {
    async fn connect(scratch: &Scratch) -> Self {
        let target = SshTarget {
            host: HOST.to_owned(),
            port: PORT,
            user: USER.to_owned(),
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

    /// Waits until an event the far end has to produce shows up, or gives up loudly.
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

/// **Where the far end's first prompt goes**, measured rather than reasoned about.
///
/// Reported by the user on 2026-08-26: "on connection, I heard no prompt." The answer that
/// an unintegrated session is silent by design was rejected, correctly — cmd is unintegrated
/// for *output* and its prompt is still heard, so "unintegrated" plainly does not mean
/// silent across this product.
///
/// **What it measured, and what it measures now** (spec B6.2, then B9.5). Filed as roadmap
/// 27.4, this recorded two events and no output at all: `Connected`, then
/// `IntegrationUnavailable`, with the far end's prompt discarded inside the pump for having
/// fallen in no region. B6.2 made it record the login banner and the prompt, in a block
/// nobody submitted, read aloud.
///
/// **B9.5 removed the second half of that**: an SSH bash is set up now, so the sentence
/// saying it is not integrated is gone — which is the subject of
/// [`the_markers_a_session_sets_itself_up_with_cross_an_ssh_connection`] below. The prompt
/// still has to reach a listener before any of that happens, and that is what is asserted
/// here.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn what_a_session_says_when_it_has_just_connected() {
    let scratch = Scratch::new();
    let pipeline = Pipeline::connect(&scratch).await;

    // Long enough for the prompt to arrive, be rendered and fall quiet — and, since B9.5,
    // for the setup to have run and the marked prompt to have been drawn behind it.
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

/// **Whether OSC 133 markers cross an SSH connection, measured** — the one question spec
/// B9.5 listed and could not answer on the machine it was implemented on, because the engine
/// this rig needs was not running there.
///
/// It matters because B9.5's decision 2 is that one mechanism serves every transport: the
/// line that sets a distribution up is the line that sets a host on the other side of the
/// world up, since Acter controls no launch arguments over SSH and never did. That decision
/// was taken on the reasoning that markers are ordinary bytes in the stream and an SSH
/// channel is a byte pipe — which is a good argument and not a measurement.
///
/// **Measured 2026-08-29** against `docker/ssh` (Debian bookworm, bash 5.2.15, OpenSSH 9.2)
/// from Windows 11, over a real `sshd` on a real channel: they cross, whole and in both
/// directions of the cycle. The far end's prompt arrives delimited — a `PromptDrawn` is an
/// `A`..`B` pair and nothing else produces one — and a command that exits 7 is announced as
/// having failed with 7, which needs the `D` the far end wrote to have survived the trip.
/// So the answer is yes for the two markers a listener actually hears.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn the_markers_a_session_sets_itself_up_with_cross_an_ssh_connection() {
    let scratch = Scratch::new();
    let pipeline = Pipeline::connect(&scratch).await;

    // The setup goes out on the far end's first drawn line and the shell re-draws its prompt
    // behind it; the marked prompt is the first evidence the far end accepted the line.
    pipeline
        .wait_until("the far end to draw a marked prompt", |seen| {
            seen.iter()
                .any(|event| matches!(event, SessionEvent::PromptDrawn { .. }))
        })
        .await;

    // A command that genuinely fails, which no echo can forge and no assumption can supply:
    // the verdict can only come from a `D;7` the far end wrote and the channel carried.
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

    // **And Acter's own line is never read aloud** (roadmap 23.12). This far end is where
    // the second cause was found: `sshd` prints `Last login: ...` before the prompt, so the
    // first *drawn* line the setup goes out on is the banner, the row B4.9 holds a
    // submission on is the banner's, and the echo of a five-hundred-character command
    // arrived as ordinary output — twice. Nothing here recognises an echo; the window Acter
    // talks to itself in is what quiets it, and every byte is still in the buffer below.
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

    // **And quieting it must never mean losing it.** Where the command is found depends on
    // what the far end did with the echo: when it lands in the `B..C` region the tracker
    // labels, it is the block's *heading*, and when a banner or a redraw puts it elsewhere it
    // is the block's output. The heading is the durable half — it is what a listener reaches
    // with F6 and then the previous-heading command — so that is what is asserted.
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
