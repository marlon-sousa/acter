//! Integration test: the whole stack against a real shell (a real `LocalPty`, `AlacrittyEngine`,
//! `SessionService` and clock), observed through the `SessionEvent`s a frontend would receive.
//!
//! Every test is `#[ignore]`d; run them with
//! `cargo test -p acter-transports --test real_session -- --ignored --nocapture`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use acter_core::{
    Announcement, Clock, CommandId, ConnectionState, EventSink, ExitCode, Key, KeyAck, KeyPress,
    PacingConfig, SessionApi, SessionEvent, SessionId, SessionService, SetUp, ShellAdapter,
    ShellFacts, ShellLaunch, ShellMarkers, Started, SubmitAck, ThisComputer, Timer, Unasked,
};
use acter_shells::{WindowsMachine, Wsl};
use acter_term::AlacrittyEngine;
use acter_transports::LocalPty;
use tokio::sync::oneshot;

const SHELL: &str = "cmd.exe";

/// Windows PowerShell 5.1; PowerShell 7 emits byte-for-byte the same marker stream.
const POWERSHELL: &str = "powershell.exe";

const COLUMNS: u16 = 80;
/// Fewer than the forty rows the flood tests print, so they scroll.
const SCREEN_LINES: u16 = 24;

const SESSION: SessionId = SessionId(1);

const PATIENCE: Duration = Duration::from_secs(20);
const DOCKER_PATIENCE: Duration = Duration::from_secs(90);

const IMAGE: &str = "alpine";

const GRACE: Duration = Duration::from_millis(200);

const SETTLE: Duration = Duration::from_millis(750);

struct RealClock {
    origin: Instant,
}

impl RealClock {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for RealClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
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
        self.0.lock().expect("recorder poisoned").push(event);
    }
}

struct RealSession {
    session: SessionService,
    events: Arc<Recorder>,
}

impl RealSession {
    fn over(launch: &ShellLaunch, markers: ShellMarkers) -> Self {
        Self::over_within(launch, markers, GRACE)
    }

    fn adapted(shell: &dyn ShellAdapter, grace: Duration) -> Self {
        Self::launched(&shell.launch(), ShellFacts::of(shell), grace)
    }

    fn over_within(launch: &ShellLaunch, markers: ShellMarkers, grace: Duration) -> Self {
        Self::launched(
            launch,
            ShellFacts {
                markers,
                eof: None,
                setup: None,
                discards_line: None,
            },
            grace,
        )
    }

    fn launched(launch: &ShellLaunch, shell: ShellFacts, grace: Duration) -> Self {
        let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
        let environment: Vec<(&str, &str)> = launch
            .environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let pty = LocalPty::spawn(&launch.program, &args, &environment, COLUMNS, SCREEN_LINES)
            .expect("a shell starts");
        let events = Arc::new(Recorder::default());
        let session = SessionService::start(
            Box::new(pty),
            Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
            Arc::new(RealClock::new()) as Arc<dyn Clock>,
            PacingConfig {
                integration_grace: grace,
                ..PacingConfig::default()
            },
            shell,
        );
        session.attach_session(SESSION, Arc::clone(&events) as Arc<dyn EventSink>);
        Self { session, events }
    }

    fn cmd() -> Self {
        let launch = ShellLaunch {
            environment: Vec::new(),
            ..acter_shells::adapter_for(SHELL).launch()
        };
        Self::over(&launch, ShellMarkers::Full)
    }

    fn marked() -> Self {
        Self::adapted(acter_shells::adapter_for(SHELL).as_ref(), GRACE)
    }

    fn submit(&self, line: &str) -> CommandId {
        match self.session.submit_command(SESSION, line) {
            SubmitAck::Accepted { command_id } => command_id,
            SubmitAck::NotConnected => panic!("a running session accepts a line"),
        }
    }

    fn output_of(&self, command_id: CommandId) -> String {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .filter_map(|event| match event {
                SessionEvent::Output {
                    command_id: at,
                    text,
                    ..
                } if *at == command_id => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// `None` if the block never opened, `Some(None)` if it opened without a command line.
    fn heading_of(&self, command_id: CommandId) -> Option<Option<String>> {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .find_map(|event| match event {
                SessionEvent::CommandStarted {
                    command_id: at,
                    command_line,
                } if *at == command_id => Some(command_line.clone()),
                _ => None,
            })
    }

    fn ctrl_c(&self) {
        self.session.send_key(
            SESSION,
            KeyPress {
                key: Key::Char('c'),
                ctrl: true,
                shift: false,
                alt: false,
            },
        );
    }

    /// Windows PowerShell 5.1 under a 200 ms grace is flagged `IntegrationUnavailable` before
    /// its first marker arrives, so this runs under the shipped grace.
    fn powershell() -> Self {
        Self::adapted(
            acter_shells::adapter_for(POWERSHELL).as_ref(),
            PacingConfig::default().integration_grace,
        )
    }

    fn ctrl_d(&self) -> KeyAck {
        self.session.send_key(
            SESSION,
            KeyPress {
                key: Key::Char('d'),
                ctrl: true,
                shift: false,
                alt: false,
            },
        )
    }

    fn finished(&self, command_id: CommandId) -> bool {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .any(|event| {
                matches!(
                    event,
                    SessionEvent::CommandFinished { command_id: at } if *at == command_id
                )
            })
    }

    fn rendered(&self) -> String {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .filter_map(|event| match event {
                SessionEvent::Output { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn said_aloud(&self) -> String {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .filter_map(|event| match event {
                SessionEvent::Announce {
                    announcement: Announcement::ReadAloud { text },
                    ..
                } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    async fn until(&self, command_id: CommandId, wanted: &str, patience: Duration) -> String {
        let deadline = Instant::now() + patience;
        loop {
            if self.output_of(command_id).contains(wanted) {
                tokio::time::sleep(SETTLE).await;
                return self.output_of(command_id);
            }
            if Instant::now() >= deadline {
                panic!(
                    "never saw {wanted:?} in {command_id:?}. What the session said was:\n{}",
                    self.rendered()
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn until_said(&self, patience: Duration, wanted: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + patience;
        loop {
            if wanted(&self.rendered()) {
                tokio::time::sleep(SETTLE).await;
                return self.rendered();
            }
            if Instant::now() >= deadline {
                panic!(
                    "the session never said it. What it did say was:\n{}",
                    self.rendered()
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn flagged(&self) {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if self
                .events
                .0
                .lock()
                .expect("recorder poisoned")
                .contains(&SessionEvent::IntegrationUnavailable)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("a marker-less shell was never flagged as unintegrated");
    }
}

const SLOW_CONSUMER: &str = concat!(
    r#"powershell -NoProfile -Command "[Console]::Write([char]27 + '[6n'); "#,
    r#"Start-Sleep -Seconds 6""#
);

const ROWS: u32 = 40;
const ROW_PREFIX: &str = "acter-row-";
const AFTER: &str = "acter-second-block";

fn only_its_own(after: &str) {
    assert!(
        after.contains(AFTER),
        "the second block has its own output: {after:?}"
    );
    assert!(
        !after.contains(ROW_PREFIX),
        "and not one row of the command that scrolled away under it: {after:?}"
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_real_shells_flood_does_not_scroll_into_the_next_block() {
    let session = RealSession::cmd();
    session.flagged().await;

    let flood = session.submit(&format!(
        "for /l %i in (1,1,{ROWS}) do @echo {ROW_PREFIX}%i"
    ));
    session
        .until(flood, &format!("{ROW_PREFIX}{ROWS}"), PATIENCE)
        .await;

    let next = session.submit(&format!("echo {AFTER}"));
    let after = session.until(next, AFTER, PATIENCE).await;

    only_its_own(&after);
}

#[tokio::test]
#[ignore = "spawns a real shell and a container"]
async fn a_shell_inside_a_shell_does_not_mangle_the_buffer() {
    if !docker_is_available() || !image_is_present() {
        println!("skipped: Docker is not available on this machine");
        return;
    }

    let session = RealSession::cmd();
    session.flagged().await;

    // Until `sh` has drawn `/ #`, a submitted line can still be read and answered by the
    // outer `cmd.exe`.
    let enter = session.submit(&format!("docker run -it --rm {IMAGE} sh"));
    session.until(enter, "/ #", DOCKER_PATIENCE).await;

    let flood = session.submit(&format!(
        "i=1; while [ $i -le {ROWS} ]; do echo {ROW_PREFIX}$i; i=$((i+1)); done"
    ));
    session
        .until(flood, &format!("{ROW_PREFIX}{ROWS}"), DOCKER_PATIENCE)
        .await;

    let next = session.submit(&format!("echo {AFTER}"));
    let after = session.until(next, AFTER, DOCKER_PATIENCE).await;

    only_its_own(&after);

    session.submit("exit");
}

/// Asks the daemon, because a `docker` client on `PATH` with no daemon behind it hangs the test.
fn docker_is_available() -> bool {
    docker(&["info", "--format", "{{.ServerVersion}}"])
}

/// Pulls before the session starts, so the pull's progress bars never land in a block.
fn image_is_present() -> bool {
    docker(&["pull", "--quiet", IMAGE])
}

fn docker(args: &[&str]) -> bool {
    std::process::Command::new("docker")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Depends on ConPTY echoing the written line onto the row `cmd.exe` drew its prompt on.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_real_shells_echo_opens_the_block_and_becomes_its_heading() {
    let session = RealSession::cmd();
    session.flagged().await;

    let line = format!("echo {AFTER}");
    let command = session.submit(&line);
    let output = session.until(command, AFTER, PATIENCE).await;

    assert_eq!(
        session.heading_of(command),
        Some(Some(line.clone())),
        "the heading is the echo the shell produced: {:?}",
        session.heading_of(command)
    );
    assert!(
        !output.contains(&line),
        "and the command line is not also the block's first content line: {output:?}"
    );
}

/// `ping` holds the console without reading it, and `cmd.exe` does not discard typed-ahead
/// input on Ctrl+C, so both queued lines run at once after the interrupt.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_backlog_released_by_an_interrupt_fills_its_own_blocks() {
    let session = RealSession::cmd();
    session.flagged().await;

    let held = session.submit("ping -n 20 127.0.0.1");
    session.until(held, "Pinging", PATIENCE).await;

    let queued: Vec<CommandId> = (1..=2)
        .map(|n| session.submit(&format!("echo acter-backlog-{n}")))
        .collect();
    tokio::time::sleep(Duration::from_millis(750)).await;

    session.ctrl_c();
    session.until(queued[1], "acter-backlog-2", PATIENCE).await;

    for (index, command) in queued.iter().enumerate() {
        let wanted = format!("acter-backlog-{}", index + 1);
        assert!(
            session.output_of(*command).contains(&wanted),
            "each released submission holds its own output, not its sibling's: \
             {wanted} was not in {:?}",
            session.output_of(*command)
        );
    }
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_real_cmd_carries_its_own_prompt_markers() {
    let session = RealSession::marked();

    let command = session.submit("echo acter-marked-line");
    let output = session.until(command, "acter-marked-line", PATIENCE).await;

    assert_eq!(
        session.heading_of(command),
        Some(Some("echo acter-marked-line".to_owned())),
        "the block is named by what the far end echoed, and the echo is what opened it"
    );
    assert!(
        !session
            .events
            .0
            .lock()
            .expect("recorder poisoned")
            .contains(&SessionEvent::IntegrationUnavailable),
        "a shell whose prompt carries A and B is integrated, not flagged"
    );
    assert!(
        output.contains('>'),
        "the returning prompt is the last thing the block says — the only ending a shell \
         with no exit code has: {output:?}"
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_submission_behind_an_unread_device_query_answer_still_runs() {
    let session = RealSession::marked();

    let slow = session.submit(SLOW_CONSUMER);
    session.until(slow, ">", DOCKER_PATIENCE).await;

    let command = session.submit("echo acter-second-line");
    let output = session.until(command, "acter-second-line", PATIENCE).await;

    assert!(
        !session.rendered().contains("not recognized"),
        "the line the user submitted is the line the shell ran: {output:?}"
    );
    assert!(
        !session.rendered().contains("^["),
        "and this pump's own answer is never in the buffer as text: {:?}",
        session.rendered()
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn no_command_in_a_marked_session_reads_the_typed_line_back() {
    let session = RealSession::marked();

    let first = session.submit("echo acter-alpha");
    session.until(first, "acter-alpha", PATIENCE).await;

    let second = session.submit("echo acter-bravo");
    let output = session.until(second, "acter-bravo", PATIENCE).await;

    // Asked of the whole session, because a leaked echo lands in the previous command's block.
    let said = session.rendered();
    assert!(
        !said.contains("echo acter-bravo"),
        "the command line is never read back at the user, in any block: {said:?}"
    );
    assert!(
        output.contains('>'),
        "and the returning prompt is still the last thing it says: {output:?}"
    );
    assert_eq!(
        session.heading_of(second),
        Some(Some("echo acter-bravo".to_owned())),
        "the command line belongs in the heading and nowhere else"
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn no_command_in_an_unintegrated_session_reads_the_typed_line_back() {
    let session = RealSession::cmd();
    session.flagged().await;

    let first = session.submit("echo acter-alpha");
    session.until(first, "acter-alpha", PATIENCE).await;

    let second = session.submit("echo acter-bravo");
    let output = session.until(second, "acter-bravo", PATIENCE).await;

    let said = session.rendered();
    assert!(
        !said.contains("echo acter-bravo") && !said.contains("echo acter-alpha"),
        "no command line is read back at the user, in any block: {said:?}"
    );
    assert!(
        output.contains('>'),
        "and the returning prompt is still the last thing it says: {output:?}"
    );
    assert_eq!(
        session.heading_of(second),
        Some(Some("echo acter-bravo".to_owned())),
        "the command line belongs in the heading and nowhere else"
    );
}

/// Inside `docker run -it` the outer command's `C..D` never closes, so the echo of every
/// line typed into the container lands in that one open block.
#[tokio::test]
#[ignore = "spawns a real shell and a container"]
async fn a_line_typed_into_a_container_is_not_read_back() {
    if !docker_is_available() || !image_is_present() {
        println!("skipped: Docker is not available on this machine");
        return;
    }

    let session = RealSession::marked();

    let enter = session.submit(&format!("docker run -it --rm {IMAGE} sh"));
    session.until(enter, "/ #", DOCKER_PATIENCE).await;

    let inside = session.submit(&format!("echo {AFTER}"));
    session.until(inside, AFTER, DOCKER_PATIENCE).await;

    let said = session.rendered();
    assert!(
        !said.contains(&format!("echo {AFTER}")),
        "the line typed into the container is not read back at the user: {said:?}"
    );
    assert!(
        said.contains("/ #"),
        "and the container's own prompt still is: {said:?}"
    );

    session.submit("exit");
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_bare_enter_brings_the_prompt_back() {
    let session = RealSession::cmd();
    session.flagged().await;

    let first = session.submit("echo acter-alpha");
    let before = session
        .until(first, "acter-alpha", PATIENCE)
        .await
        .matches('>')
        .count();

    session.submit("");

    let said = session
        .until_said(PATIENCE, |said| said.matches('>').count() > before)
        .await;
    assert!(
        said.matches('>').count() > before,
        "the shell drew its prompt again, and the user hears where they are: {said:?}"
    );
}

fn bash_setup() -> String {
    acter_shells::setup_for(Some("bash"))
        .expect("bash has a measured setup")
        .line
}

const WSL: &str = "wsl.exe";

const WSL_PATIENCE: Duration = Duration::from_secs(60);

impl RealSession {
    /// Waits for the setup line's block to end, because a line submitted before then races
    /// the setup line.
    async fn wsl() -> Self {
        let adapter = Wsl::new(WSL, WindowsMachine::new().login_shell(None).as_deref());
        // A WSL distribution takes seconds to start, so a 200 ms grace flags the session
        // before bash draws a prompt.
        let session = Self::adapted(&adapter, PacingConfig::default().integration_grace);
        session.set_up(&bash_setup()).await;
        session
    }

    fn setup_block(&self, line: &str) -> Option<CommandId> {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .find_map(|event| match event {
                SessionEvent::CommandStarted {
                    command_id,
                    command_line: Some(said),
                } if said == line => Some(*command_id),
                _ => None,
            })
    }

    async fn set_up(&self, line: &str) {
        let deadline = Instant::now() + WSL_PATIENCE;
        loop {
            if let Some(command_id) = self.setup_block(line)
                && self.ended(command_id)
            {
                tokio::time::sleep(SETTLE).await;
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the session was never set up. What it said was:\n{:?}",
                self.events.0.lock().expect("recorder poisoned")
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// `None` means no failure was announced for the block, which is all a success produces.
    fn failure_of(&self, command_id: CommandId) -> Option<ExitCode> {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .find_map(|event| match event {
                SessionEvent::Announce {
                    command_id: at,
                    announcement: Announcement::Failed { exit_code },
                } if *at == command_id => Some(*exit_code),
                _ => None,
            })
    }

    fn ended(&self, command_id: CommandId) -> bool {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .any(|event| match event {
                SessionEvent::CommandFinished { command_id: at }
                | SessionEvent::CommandInterrupted { command_id: at } => *at == command_id,
                _ => false,
            })
    }

    async fn became_zsh(&self) {
        let deadline = Instant::now() + WSL_PATIENCE;
        loop {
            self.submit("echo acter-shell-is-${ZSH_VERSION:-none}");
            tokio::time::sleep(SETTLE).await;
            if self.rendered().contains("acter-shell-is-5") {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the session never became zsh. What it said was:\n{}",
                self.rendered()
            );
        }
    }

    async fn wsl_until(&self, command_id: CommandId, wanted: &str) -> String {
        self.until(command_id, wanted, WSL_PATIENCE).await
    }
}

/// Asks for a distribution, because every Windows 11 install ships `wsl.exe` and a client
/// with no distribution behind it hangs on a prompt that never comes.
fn wsl_is_available() -> bool {
    WindowsMachine::new().wsl_distributions().is_ok()
}

#[tokio::test]
#[ignore = "spawns a real shell and needs a WSL distribution installed"]
async fn a_real_bash_under_wsl_marks_the_boundaries_of_the_command_it_ran() {
    if !wsl_is_available() {
        println!("skipped: this machine has no WSL distribution");
        return;
    }

    let session = RealSession::wsl().await;

    let line = "echo acter-under-wsl";
    let command = session.submit(line);
    let output = session.wsl_until(command, "acter-under-wsl").await;

    assert_eq!(
        session.heading_of(command),
        Some(Some(line.to_owned())),
        "the block is named by the line bash echoed between B and C"
    );
    assert!(
        output.contains("acter-under-wsl"),
        "and holds what the command printed: {output:?}"
    );
    assert!(
        !output.contains(line),
        "the command line belongs in the heading and nowhere else: {output:?}"
    );
    assert!(
        session.ended(command),
        "a shell that emits D closes its own block, rather than leaving it open until \
         the next prompt"
    );
    assert!(
        !session
            .events
            .0
            .lock()
            .expect("recorder poisoned")
            .contains(&SessionEvent::IntegrationUnavailable),
        "a session whose PROMPT_COMMAND survived the user's .bashrc is integrated, and \
         within the grace period the application really ships"
    );
}

fn zsh_setup() -> String {
    acter_shells::setup_for(Some("zsh"))
        .expect("zsh has a measured setup since B5.8")
        .line
}

/// zsh 5.9 on Ubuntu 24.04 under WSL: its OSC 133 markers and `D` exit code cross `wsl.exe`'s
/// pseudoconsole.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real shell and needs a WSL distribution with zsh installed"]
async fn a_real_zsh_under_wsl_marks_the_boundaries_of_the_command_it_ran() {
    if !wsl_is_available() {
        println!("skipped: this machine has no WSL distribution");
        return;
    }

    let session = RealSession::wsl().await;

    session.submit("exec zsh");
    session.became_zsh().await;

    let line = zsh_setup();
    let setting_up = session.submit(&line);
    session
        .until_said(WSL_PATIENCE, |_| session.ended(setting_up))
        .await;

    let ran = session.submit("echo acter-under-wsl-zsh");
    let output = session.wsl_until(ran, "acter-under-wsl-zsh").await;

    assert_eq!(
        session.heading_of(ran),
        Some(Some("echo acter-under-wsl-zsh".to_owned())),
        "the block is named by the line zsh echoed between B and C"
    );
    assert!(
        session.ended(ran),
        "a shell that emits D closes its own block rather than leaving it open until the \
         next prompt: {output:?}"
    );

    let failed = session.submit("(exit 7)");
    session
        .until_said(WSL_PATIENCE, |_| session.failure_of(failed).is_some())
        .await;

    assert_eq!(
        session.failure_of(failed),
        Some(ExitCode(7)),
        "zsh's own verdict crossed WSL: {}",
        session.rendered()
    );
}
#[tokio::test]
#[ignore = "asks a real wsl.exe and needs a WSL distribution installed"]
async fn a_real_distribution_says_what_shell_it_runs() {
    if !wsl_is_available() {
        println!("skipped: this machine has no WSL distribution");
        return;
    }

    let said = WindowsMachine::new()
        .login_shell(None)
        .expect("a distribution that starts answers what its account runs");

    println!("the default distribution runs {said}");
    assert!(
        !said.contains('/') && !said.contains(char::is_whitespace),
        "a name is what a listener hears, not a path: {said:?}"
    );
    assert!(
        !said.contains('$'),
        "an unexpanded variable is not a shell name: {said:?}"
    );
}

#[tokio::test]
#[ignore = "spawns a real shell and needs a WSL distribution installed"]
async fn nothing_the_probe_asked_reaches_the_session_a_listener_reads() {
    if !wsl_is_available() {
        println!("skipped: this machine has no WSL distribution");
        return;
    }

    let session = RealSession::wsl().await;

    let command = session.submit("echo acter-probe-check");
    session.wsl_until(command, "acter-probe-check").await;

    let rendered = session.rendered();
    for trace in ["getent", "passwd", "id -un", "$SHELL"] {
        assert!(
            !rendered.contains(trace),
            "the probe asked on its own invocation, so {trace:?} is in no buffer a listener \
             reads: {rendered:?}"
        );
    }
}

#[tokio::test]
#[ignore = "spawns a real shell and needs a WSL distribution installed"]
async fn a_command_that_fails_under_wsl_is_announced_with_the_code_it_failed_with() {
    if !wsl_is_available() {
        println!("skipped: this machine has no WSL distribution");
        return;
    }

    let session = RealSession::wsl().await;

    let opener = session.submit("echo acter-before-the-failure");
    session.wsl_until(opener, "acter-before-the-failure").await;
    assert_eq!(
        session.failure_of(opener),
        None,
        "a command that succeeded is not announced as a failure"
    );

    let failing = session.submit("(exit 3)");
    session
        .until_said(WSL_PATIENCE, |_| session.failure_of(failing).is_some())
        .await;

    assert_eq!(
        session.failure_of(failing),
        Some(ExitCode(3)),
        "the code bash reported in its D marker is the code the user is told"
    );
}

/// `0x03` crosses `wsl.exe`, bash's line discipline turns it into `SIGINT`, and exit 130
/// arrives in a `D` marker.
#[tokio::test]
#[ignore = "spawns a real shell and needs a WSL distribution installed"]
async fn an_interrupt_stops_a_program_inside_a_wsl_distribution() {
    if !wsl_is_available() {
        println!("skipped: this machine has no WSL distribution");
        return;
    }

    let session = RealSession::wsl().await;

    let opener = session.submit("echo acter-before-the-interrupt");
    session
        .wsl_until(opener, "acter-before-the-interrupt")
        .await;

    let held = session.submit("while true; do sleep 1; done");
    // The loop prints nothing to wait for, so this gives bash time to start running it.
    tokio::time::sleep(Duration::from_secs(2)).await;

    session.ctrl_c();

    let after = session.submit("echo acter-still-alive");
    let output = session.wsl_until(after, "acter-still-alive").await;

    assert!(
        session.ended(held),
        "the loop's block was closed rather than left open forever"
    );
    assert!(
        output.contains("acter-still-alive"),
        "and the session goes on working afterwards: {output:?}"
    );
}

/// Hashes the startup files only, because bash appends to `.bash_history` on its own.
#[tokio::test]
#[ignore = "spawns a real shell and needs a WSL distribution installed"]
async fn a_wsl_session_leaves_the_distributions_own_files_alone() {
    if !wsl_is_available() {
        println!("skipped: this machine has no WSL distribution");
        return;
    }

    const HASH: &str = "md5sum ~/.bashrc ~/.profile | md5sum";
    let untouched = outside_the_session(HASH);
    let session = RealSession::wsl().await;

    let command = session.submit(HASH);
    let output = session.wsl_until(command, &untouched).await;

    assert!(
        output.contains(&untouched),
        "the startup files are the ones that were there before Acter started: expected \
         {untouched:?} in {output:?}"
    );
    assert_eq!(
        outside_the_session(HASH),
        untouched,
        "and they are still those after the session has run"
    );
}

fn outside_the_session(command: &str) -> String {
    let answered = std::process::Command::new(WSL)
        .args(["--", "bash", "-c", command])
        .output()
        .expect("wsl answers");
    String::from_utf8_lossy(&answered.stdout)
        .split_whitespace()
        .next()
        .expect("md5sum prints a hash")
        .to_owned()
}

/// Runs against `cmd.exe`, which draws its prompt in tens of milliseconds; Windows PowerShell's
/// cold start on a loaded CI runner outlasted the five-second head start and reddened the test.
/// Waiting longer after attaching is no fix: a prompt that arrives after the attach arrives
/// live and the test stops exercising replay.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_frontend_that_attaches_late_is_told_everything_it_missed() {
    let adapter = acter_shells::adapter_for(SHELL);
    let launch = adapter.launch();
    let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
    let environment: Vec<(&str, &str)> = launch
        .environment
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let pty = LocalPty::spawn(&launch.program, &args, &environment, COLUMNS, SCREEN_LINES)
        .expect("a shell starts");
    let session = SessionService::start(
        Box::new(pty),
        Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
        Arc::new(RealClock::new()) as Arc<dyn Clock>,
        PacingConfig::default(),
        ShellFacts::of(adapter.as_ref()),
    );

    tokio::time::sleep(Duration::from_secs(5)).await;

    let events = Arc::new(Recorder::default());
    session.attach_session(SESSION, Arc::clone(&events) as Arc<dyn EventSink>);
    let session = RealSession { session, events };

    let said = |session: &RealSession| -> Vec<SessionEvent> {
        session
            .events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .cloned()
            .collect()
    };

    assert!(
        said(&session).iter().any(|event| matches!(
            event,
            SessionEvent::ConnectionChanged {
                state: ConnectionState::Connected
            }
        )),
        "the window is told the session is usable, however late it asked: {:?}",
        said(&session)
    );
    assert!(
        said(&session).iter().any(|event| matches!(
            event,
            SessionEvent::Output { text, .. } if text.contains('>')
        )),
        "and the prompt drawn before it attached is not lost: {:?}",
        said(&session)
    );

    let command = session.submit("echo acter-after-a-late-attach");
    session
        .until(command, "acter-after-a-late-attach", PATIENCE)
        .await;
    tokio::time::sleep(SETTLE).await;

    assert!(
        said(&session).iter().any(|event| matches!(
            event,
            SessionEvent::Announce {
                announcement: Announcement::ReadAloud { text },
                ..
            } if text.contains("acter-after-a-late-attach")
        )),
        "output is still announced after a late attach: {:?}",
        said(&session)
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_real_powershell_reports_the_prompt_it_drew() {
    let session = RealSession::powershell();

    let command = session.submit("echo acter-before-the-prompt");
    session
        .until(command, "acter-before-the-prompt", PATIENCE)
        .await;
    tokio::time::sleep(SETTLE).await;

    let prompts: Vec<String> = session
        .events
        .0
        .lock()
        .expect("recorder poisoned")
        .iter()
        .filter_map(|event| match event {
            SessionEvent::PromptDrawn { text } => Some(text.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !prompts.is_empty(),
        "a marked session says what its prompt says, and it said: {prompts:?}"
    );
    assert!(
        prompts.iter().any(|drawn| drawn.contains("PS ")),
        "and what it says is PowerShell's own prompt: {prompts:?}"
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_real_powershell_marks_every_boundary_and_finishes_its_blocks() {
    let session = RealSession::powershell();

    let command = session.submit("echo acter-marked-line");
    let output = session.until(command, "acter-marked-line", PATIENCE).await;

    assert_eq!(
        session.heading_of(command),
        Some(Some("echo acter-marked-line".to_owned())),
        "the block is named by what the far end echoed between B and C"
    );
    assert!(
        output.contains("acter-marked-line"),
        "and holds the command's output: {output:?}"
    );
    assert!(
        !output.contains("echo acter-marked-line"),
        "and never the line the user typed, which belongs in the heading: {output:?}"
    );
    assert!(
        session.finished(command),
        "the block closed because the shell said so: {:?}",
        session.events.0.lock().expect("recorder poisoned")
    );
    assert!(
        !session
            .events
            .0
            .lock()
            .expect("recorder poisoned")
            .contains(&SessionEvent::IntegrationUnavailable),
        "a shell that marks all four boundaries is integrated, not flagged"
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn no_command_in_a_powershell_session_reads_the_typed_line_back() {
    let session = RealSession::powershell();

    let first = session.submit("echo acter-alpha");
    session.until(first, "acter-alpha", PATIENCE).await;

    let second = session.submit("echo acter-bravo");
    session.until(second, "acter-bravo", PATIENCE).await;

    let said = session.rendered();
    assert!(
        !said.contains("echo acter-bravo") && !said.contains("echo acter-alpha"),
        "no command line is read back at the user, in any block: {said:?}"
    );
    assert_eq!(
        session.heading_of(second),
        Some(Some("echo acter-bravo".to_owned())),
        "the command line belongs in the heading and nowhere else"
    );
}

/// See `crates/acter-shells/src/powershell.rs` for why `C` still fires for a line that names
/// no command.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_line_that_invokes_no_command_still_says_what_it_produced() {
    let session = RealSession::powershell();

    let command = session.submit("1..3");
    let output = session.until(command, "3", PATIENCE).await;

    for expected in ["1", "2", "3"] {
        assert!(
            output.contains(expected),
            "every line of output reached the block: {expected} missing from {output:?}"
        );
    }
    assert!(session.finished(command), "and the block closed");
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn ctrl_d_ends_a_real_powershell_session() {
    let session = RealSession::powershell();

    let first = session.submit("echo acter-before-the-end");
    session.until(first, "acter-before-the-end", PATIENCE).await;

    assert_eq!(
        session.ctrl_d(),
        KeyAck::Applied,
        "PowerShell has an end-of-input answer and it went out"
    );

    tokio::time::sleep(Duration::from_secs(3)).await;
    session.submit("echo acter-after-the-end");
    tokio::time::sleep(Duration::from_secs(3)).await;

    assert!(
        !session.rendered().contains("acter-after-the-end"),
        "the session had ended, so nothing ran in it: {:?}",
        session.rendered()
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn ctrl_d_in_a_shell_with_no_measured_answer_does_nothing() {
    let session = RealSession::marked();

    assert_eq!(session.ctrl_d(), KeyAck::Unsupported);

    let after = session.submit("echo acter-still-here");
    let output = session.until(after, "acter-still-here", PATIENCE).await;
    assert!(
        output.contains("acter-still-here"),
        "and the session is untouched: {output:?}"
    );
}

/// Only `LocalPty::drop` kills the shell, so any clone of the outgoing `Arc<dyn SessionApi>`
/// that outlives the replace keeps it running.
mod replacing_a_session {
    use std::fs::OpenOptions;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    use acter_core::{
        Chosen, ConnectApi, ConnectQuestions, ConnectService, ProfileId, RememberedConnections,
        SessionFactory, Unchecked, offered,
    };

    use super::*;

    const LOCK_PATIENCE: Duration = Duration::from_secs(30);

    static NONCE: AtomicU32 = AtomicU32::new(0);

    fn marker() -> PathBuf {
        let unique = NONCE.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("acter-b7-{}-{unique}.lock", std::process::id()))
    }

    /// True while the shell that opened this marker with `FileShare::None` is alive.
    fn held(path: &Path) -> bool {
        path.exists() && OpenOptions::new().write(true).open(path).is_err()
    }

    async fn until(patience: Duration, what: &str, ready: impl Fn() -> bool) {
        let deadline = Instant::now() + patience;
        while Instant::now() < deadline {
            if ready() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("timed out waiting for {what}");
    }

    struct LockingShells;

    impl SessionFactory for LockingShells {
        fn open(
            &self,
            chosen: &Chosen,
            _set_up: SetUp,
            _questions: &Arc<dyn ConnectQuestions>,
        ) -> Result<Started, String> {
            let ProfileId::Program { program: path } = &chosen.profile else {
                return Err("this factory only starts marker shells.".to_owned());
            };
            let snippet = format!(
                "$global:acter = [System.IO.File]::Open('{path}', 'Create', 'Write', 'None')"
            );
            let launch = ShellLaunch {
                program: POWERSHELL.to_owned(),
                args: vec![
                    "-NoProfile".to_owned(),
                    "-NoExit".to_owned(),
                    "-Command".to_owned(),
                    snippet,
                ],
                environment: Vec::new(),
            };
            let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
            let pty = LocalPty::spawn(&launch.program, &args, &[], COLUMNS, SCREEN_LINES)?;
            Ok(Started {
                session: Arc::new(SessionService::start(
                    Box::new(pty),
                    Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
                    Arc::new(RealClock::new()) as Arc<dyn Clock>,
                    PacingConfig::default(),
                    ShellFacts {
                        markers: ShellMarkers::Full,
                        eof: None,
                        setup: None,
                        discards_line: None,
                    },
                )),
                note: None,
                limit_explained: false,
            })
        }
    }

    #[tokio::test]
    #[ignore = "spawns two real shells"]
    async fn connecting_twice_leaves_exactly_one_shell_running() {
        let first = marker();
        let second = marker();
        let service = ConnectService::new(
            Arc::new(LockingShells),
            Arc::new(WindowsMachine::new()),
            Arc::new(Unchecked),
            offered("windows").to_vec(),
            Vec::new(),
            Arc::new(RememberedConnections::default()),
        );

        service
            .use_profile(
                &ProfileId::Program {
                    program: first.display().to_string(),
                },
                SetUp::Yes,
                None,
                &(Arc::new(Unasked) as Arc<dyn ConnectQuestions>),
            )
            .expect("the first shell starts");
        until(LOCK_PATIENCE, "the first shell to take its lock", || {
            held(&first)
        })
        .await;

        service
            .use_profile(
                &ProfileId::Program {
                    program: second.display().to_string(),
                },
                SetUp::Yes,
                None,
                &(Arc::new(Unasked) as Arc<dyn ConnectQuestions>),
            )
            .expect("the second shell starts");
        until(LOCK_PATIENCE, "the second shell to take its lock", || {
            held(&second)
        })
        .await;

        until(LOCK_PATIENCE, "the replaced shell to exit", || {
            !held(&first)
        })
        .await;
        assert!(
            held(&second),
            "and the one the user is now on is still running"
        );

        drop(service);
        until(
            LOCK_PATIENCE,
            "the last shell to exit with the window",
            || !held(&second),
        )
        .await;

        let _ = std::fs::remove_file(&first);
        let _ = std::fs::remove_file(&second);
    }
}

#[cfg(windows)]
mod what_this_machine_actually_has {
    use std::path::Path;

    use acter_core::{
        Chosen, ConnectApi, ConnectQuestions, ConnectService, ConnectionKind, ProfileId,
        RememberedConnections, SessionFactory, offered,
    };
    use acter_shells::{WindowsTrust, adapter_for};

    use super::*;

    struct WhateverWasChosen;

    impl SessionFactory for WhateverWasChosen {
        fn open(
            &self,
            chosen: &Chosen,
            _set_up: SetUp,
            _questions: &Arc<dyn ConnectQuestions>,
        ) -> Result<Started, String> {
            let program = chosen
                .program
                .as_ref()
                .ok_or("this factory only starts a file that was resolved.")?;
            let named = program.to_string_lossy().into_owned();
            let shell = adapter_for(&named);
            let launch = shell.launch();
            let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
            let environment: Vec<(&str, &str)> = launch
                .environment
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            let pty = LocalPty::spawn(&launch.program, &args, &environment, COLUMNS, SCREEN_LINES)?;
            Ok(Started {
                session: Arc::new(SessionService::start(
                    Box::new(pty),
                    Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
                    Arc::new(RealClock::new()) as Arc<dyn Clock>,
                    PacingConfig::default(),
                    ShellFacts::of(shell.as_ref()),
                )),
                note: None,
                limit_explained: false,
            })
        }
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and asks Windows about its signature"]
    async fn connecting_to_the_shell_windows_ships_verifies_it_and_says_nothing_new() {
        let service = ConnectService::new(
            Arc::new(WhateverWasChosen),
            Arc::new(WindowsMachine::new()),
            Arc::new(WindowsTrust::new()),
            offered("windows").to_vec(),
            Vec::new(),
            Arc::new(RememberedConnections::default()),
        );

        let connected = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &(Arc::new(Unasked) as Arc<dyn ConnectQuestions>),
            )
            .expect("cmd.exe is signed by Microsoft, so nobody has to be asked about it");

        assert_eq!(connected.label, "Command Prompt");
        assert_eq!(
            connected.note, None,
            "a verdict nobody needs to act on is not an announcement"
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and asks Windows about its signature"]
    async fn the_row_the_list_offers_names_the_file_that_gets_started() {
        let service = ConnectService::new(
            Arc::new(WhateverWasChosen),
            Arc::new(WindowsMachine::new()),
            Arc::new(WindowsTrust::new()),
            offered("windows").to_vec(),
            Vec::new(),
            Arc::new(RememberedConnections::default()),
        );

        let listed = service.connectable();
        let cmd = listed
            .iter()
            .find(|row| row.label == "Command Prompt")
            .expect("every Windows machine has it");
        let ProfileId::Install { program, .. } = &cmd.id else {
            panic!("the row names the file the list resolved: {:?}", cmd.id);
        };
        assert!(
            Path::new(program).is_file(),
            "and it is a file that is really there: {program}"
        );

        service
            .use_profile(
                &cmd.id,
                SetUp::Yes,
                None,
                &(Arc::new(Unasked) as Arc<dyn ConnectQuestions>),
            )
            .expect("choosing the row starts it");
    }
}

mod the_session_is_set_up_after_it_is_established {
    use std::fs::{create_dir_all, write};

    use super::*;

    /// Named, because WSL's default distribution may be `docker-desktop`, which has no bash.
    const BASH_DISTRIBUTION: &str = "Ubuntu";

    /// Docker Desktop's service distribution, whose `/bin/sh` is busybox.
    const SH_DISTRIBUTION: &str = "docker-desktop";

    fn has(distribution: &str) -> bool {
        WindowsMachine::new()
            .wsl_distributions()
            .is_ok_and(|installed| installed.iter().any(|named| named == distribution))
    }

    /// Writes on the Windows side and answers with the `/mnt` path, so nothing is left inside
    /// the distribution.
    fn rcfile(named: &str, contents: &str) -> String {
        let mut at = std::env::temp_dir();
        at.push("acter-b9-5");
        create_dir_all(&at).expect("a directory for the rcfile");
        at.push(format!("rc-{named}"));
        write(&at, contents).expect("an rcfile the distribution can read");

        let windows = at.to_string_lossy().replace('\\', "/");
        let (drive, rest) = windows.split_once(':').expect("an absolute Windows path");
        format!("/mnt/{}{rest}", drive.to_lowercase())
    }

    impl RealSession {
        async fn wsl_with(rc: &str) -> Self {
            let launch = ShellLaunch {
                program: WSL.to_owned(),
                args: ["-d", BASH_DISTRIBUTION, "--", "bash", "--rcfile", rc, "-i"]
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .collect(),
                environment: Vec::new(),
            };
            let session = Self::launched(
                &launch,
                ShellFacts::of(&Wsl::new(WSL, Some("bash"))),
                PacingConfig::default().integration_grace,
            );
            session.set_up(&bash_setup()).await;
            session
        }

        async fn failed_with(&self, command_id: CommandId) -> Option<ExitCode> {
            let deadline = Instant::now() + WSL_PATIENCE;
            while Instant::now() < deadline {
                if self.ended(command_id) {
                    tokio::time::sleep(SETTLE).await;
                    return self.failure_of(command_id);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            panic!(
                "{command_id:?} never ended. What the session said was:\n{:?}",
                self.events.0.lock().expect("recorder poisoned")
            );
        }
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs a WSL distribution installed"]
    async fn the_setup_opens_one_block_headed_by_the_command_and_closes_it() {
        if !wsl_is_available() {
            println!("skipped: this machine has no WSL distribution");
            return;
        }

        let session = RealSession::wsl().await;

        let setup = session
            .setup_block(&bash_setup())
            .expect("the setup opened a block headed by the command verbatim");
        assert!(
            session.finished(setup),
            "and it closed, with an exit code, before the user's first command"
        );
        assert_eq!(
            session.failure_of(setup),
            None,
            "an assignment succeeds, and a success is not announced (A6, decision 2)"
        );
        assert_eq!(
            session.output_of(setup),
            "",
            "a successful setup prints nothing, so the block is silent as well as closed"
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs a WSL distribution installed"]
    async fn nothing_of_the_setup_is_read_aloud() {
        if !wsl_is_available() {
            println!("skipped: this machine has no WSL distribution");
            return;
        }

        let session = RealSession::wsl().await;

        let said: Vec<String> = session
            .events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .filter_map(|event| match event {
                SessionEvent::Announce {
                    announcement: Announcement::ReadAloud { text },
                    ..
                } => Some(text.clone()),
                SessionEvent::PromptDrawn { text } => Some(text.clone()),
                _ => None,
            })
            .collect();

        for spoken in &said {
            assert!(
                !spoken.contains("PROMPT_COMMAND"),
                "Acter's own command reached the listener: {spoken:?}"
            );
        }
        assert!(
            !session.rendered().contains("__acter_prompt"),
            "nor the buffer, as output: {:?}",
            session.rendered()
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs a WSL distribution installed"]
    async fn a_session_that_has_just_connected_has_nothing_to_stop() {
        if !wsl_is_available() {
            println!("skipped: this machine has no WSL distribution");
            return;
        }

        let session = RealSession::wsl().await;

        assert_eq!(
            session.session.send_key(
                SESSION,
                KeyPress {
                    key: Key::Char('c'),
                    ctrl: true,
                    shift: false,
                    alt: false,
                },
            ),
            KeyAck::NothingToActOn,
            "a listener is never told they stopped a command nobody ran"
        );
        assert_eq!(
            session.ctrl_d(),
            KeyAck::Unsupported,
            "and bash under WSL still has no measured end-of-input byte (roadmap 23.8)"
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs a WSL distribution installed"]
    async fn a_hook_prepended_before_ours_still_reports_the_code_the_command_failed_with() {
        if !has(BASH_DISTRIBUTION) {
            println!("skipped: this machine has no {BASH_DISTRIBUTION}");
            return;
        }

        let rc = rcfile(
            "prepend",
            "PS1='rc$ '\nPROMPT_COMMAND=\"true; $PROMPT_COMMAND\"\n",
        );
        let session = RealSession::wsl_with(&rc).await;

        let failing = session.submit("(exit 7)");

        assert_eq!(
            session.failed_with(failing).await,
            Some(ExitCode(7)),
            "the code the command actually failed with, not the hook's"
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs a WSL distribution installed"]
    async fn a_hook_appended_after_ours_no_longer_steals_the_marker() {
        if !has(BASH_DISTRIBUTION) {
            println!("skipped: this machine has no {BASH_DISTRIBUTION}");
            return;
        }

        let rc = rcfile(
            "append",
            "PS1='rc$ '\nPROMPT_COMMAND=\"$PROMPT_COMMAND; true\"\n",
        );
        let session = RealSession::wsl_with(&rc).await;

        let command = session.submit("echo acter-appended");
        let output = session.wsl_until(command, "acter-appended").await;

        assert!(output.contains("acter-appended"));
        assert_eq!(
            session.heading_of(command),
            Some(Some("echo acter-appended".to_owned())),
            "the block is the command the user ran, opened where their output began"
        );
        assert_eq!(
            session.failed_with(command).await,
            None,
            "and a command that worked is not announced as having failed"
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs a WSL distribution installed"]
    async fn a_hook_that_rebuilds_the_prompt_is_re_wrapped_every_time() {
        if !has(BASH_DISTRIBUTION) {
            println!("skipped: this machine has no {BASH_DISTRIBUTION}");
            return;
        }

        let rc = rcfile(
            "rebuild",
            "PS1='before$ '\n__mine() { PS1='rebuilt$ '; }\nPROMPT_COMMAND=__mine\n",
        );
        let session = RealSession::wsl_with(&rc).await;

        // Two rounds, because a one-shot guard keeps the boundaries past the first prompt only.
        for round in 0..2 {
            let command = session.submit(&format!("echo acter-rebuilt-{round}"));
            session
                .wsl_until(command, &format!("acter-rebuilt-{round}"))
                .await;
            assert_eq!(
                session.heading_of(command),
                Some(Some(format!("echo acter-rebuilt-{round}"))),
                "round {round} still has prompt boundaries to open its block with"
            );
        }
        assert!(
            session.rendered().contains("rebuilt$"),
            "and the rcfile's own prompt is what was rebuilt: {:?}",
            session.rendered()
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs a WSL distribution installed"]
    async fn a_dotfile_that_takes_the_prompt_hook_for_itself_is_still_set_up() {
        if !has(BASH_DISTRIBUTION) {
            println!("skipped: this machine has no {BASH_DISTRIBUTION}");
            return;
        }

        let rc = rcfile(
            "assign",
            "PS1='rc$ '\n__mine() { :; }\nPROMPT_COMMAND=__mine\n",
        );
        let session = RealSession::wsl_with(&rc).await;

        let failing = session.submit("(exit 7)");

        assert_eq!(
            session.failed_with(failing).await,
            Some(ExitCode(7)),
            "a session the old strategy could not mark at all reports a real exit code"
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs the docker-desktop distribution"]
    async fn a_distribution_running_sh_is_set_up_as_far_as_its_prompt_reaches() {
        if !has(SH_DISTRIBUTION) {
            println!("skipped: this machine has no {SH_DISTRIBUTION}");
            return;
        }

        let shell = WindowsMachine::new().login_shell(Some(SH_DISTRIBUTION));
        assert_eq!(
            shell.as_deref(),
            Some("sh"),
            "the probe is what this test is keyed on, and it answers for itself"
        );
        let adapter = Wsl::in_distribution(WSL, SH_DISTRIBUTION, shell.as_deref());
        assert_eq!(
            adapter.markers(),
            ShellMarkers::PromptCommandLineAndExitCode,
            "what its own line earns, and not bash's claim"
        );

        let session = RealSession::adapted(&adapter, PacingConfig::default().integration_grace);
        let line = acter_shells::setup_for(Some("sh"))
            .expect("sh has a measured setup")
            .line;
        session.set_up(&line).await;

        let command = session.submit("echo acter-under-sh");
        let output = session.wsl_until(command, "acter-under-sh").await;

        assert!(output.contains("acter-under-sh"));
        // `docker-desktop` can draw a prompt of seventy-six columns, which cuts the heading at
        // the row wrap, so only a prefix of the line is asserted.
        let heading = session
            .heading_of(command)
            .expect("the block was started")
            .expect("and headed");
        assert!(
            "echo acter-under-sh".starts_with(&heading) && !heading.is_empty(),
            "the heading is the submitted line, or as much of it as the row held: {heading:?}"
        );

        let failing = session.submit("(exit 7)");
        assert_eq!(
            session.failed_with(failing).await,
            Some(ExitCode(7)),
            "a shell with no post-execution hook still reports a real exit code, because              `PS1` is expanded at every prompt and `$?` is the last command's status              (roadmap 23.15)"
        );

        assert!(
            !session.said_aloud().contains("BB_ASH_VERSION"),
            "Acter's own setup command was read to the listener: {:?}",
            session.said_aloud()
        );
        // The setup line lands in the heading, or in the output when busybox redraws the echo.
        let headed = session.setup_block(&line).is_some();
        assert!(
            headed || session.rendered().contains("BB_ASH_VERSION"),
            "the disclosure has to be readable back, and it is in neither the heading nor              the buffer: {:?}",
            session.rendered()
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs the docker-desktop distribution"]
    async fn a_command_that_does_not_exist_still_says_so_in_sh() {
        if !has(SH_DISTRIBUTION) {
            println!("skipped: this machine has no {SH_DISTRIBUTION}");
            return;
        }

        let shell = WindowsMachine::new().login_shell(Some(SH_DISTRIBUTION));
        let adapter = Wsl::in_distribution(WSL, SH_DISTRIBUTION, shell.as_deref());
        let session = RealSession::adapted(&adapter, PacingConfig::default().integration_grace);
        let line = acter_shells::setup_for(Some("sh"))
            .expect("sh has a measured setup")
            .line;
        session.set_up(&line).await;

        let command = session.submit("acter-no-such-command");
        tokio::time::sleep(Duration::from_secs(5)).await;

        println!("--- what sh said about a command that does not exist ---");
        for event in session.events.0.lock().expect("recorder poisoned").iter() {
            println!("  {event:?}");
        }
        println!("--- heading: {:?} ---", session.heading_of(command));

        assert!(
            session.rendered().contains("not found"),
            "a listener is told the command does not exist: {:?}",
            session.rendered()
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs the docker-desktop distribution"]
    async fn a_long_command_that_does_not_exist_still_says_so_in_sh() {
        if !has(SH_DISTRIBUTION) {
            println!("skipped: this machine has no {SH_DISTRIBUTION}");
            return;
        }

        let shell = WindowsMachine::new().login_shell(Some(SH_DISTRIBUTION));
        let adapter = Wsl::in_distribution(WSL, SH_DISTRIBUTION, shell.as_deref());
        let session = RealSession::adapted(&adapter, PacingConfig::default().integration_grace);
        let line = acter_shells::setup_for(Some("sh"))
            .expect("sh has a measured setup")
            .line;
        session.set_up(&line).await;

        // How much of the heading survives depends on the far end's prompt width, so the length
        // is printed rather than pinned.
        let fits = "acter-no-such-command-0123456789a123456789";
        let inside = session.submit(fits);
        session.wsl_until(inside, "not found").await;

        let long = "acter-no-such-command-0123456789a123456789b123456789c123456";
        let command = session.submit(long);
        tokio::time::sleep(Duration::from_secs(5)).await;

        println!("--- what sh said about a long command that does not exist ---");
        for event in session.events.0.lock().expect("recorder poisoned").iter() {
            println!("  {event:?}");
        }
        println!(
            "--- heading, fits in the row: {:?} ---",
            session.heading_of(inside)
        );
        println!(
            "--- heading, genuinely wraps: {:?} ---",
            session.heading_of(command)
        );

        let heading = session
            .heading_of(inside)
            .expect("the block was started")
            .expect("and headed");
        println!(
            "--- of {} characters, {} survived ---",
            fits.len(),
            heading.len()
        );
        assert!(
            fits.starts_with(&heading) && !heading.is_empty(),
            "the heading is the line as far as the row held it: {heading:?}"
        );
        assert!(
            session.rendered().contains("not found"),
            "a listener is told the command does not exist: {:?}",
            session.rendered()
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs the docker-desktop distribution"]
    async fn a_refused_sh_session_still_heads_every_command() {
        if !has(SH_DISTRIBUTION) {
            println!("skipped: this machine has no {SH_DISTRIBUTION}");
            return;
        }

        let shell = WindowsMachine::new().login_shell(Some(SH_DISTRIBUTION));
        let adapter = Wsl::in_distribution(WSL, SH_DISTRIBUTION, shell.as_deref());
        let session = RealSession::launched(
            &adapter.launch(),
            ShellFacts::of(&adapter).declined(),
            PacingConfig::default().integration_grace,
        );

        // A line submitted before the prompt is drawn has no pending row to hold, so its echo
        // is published as output.
        session
            .until_said(WSL_PATIENCE, |said| said.contains('#'))
            .await;

        let short = session.submit("lsa");
        session.wsl_until(short, "not found").await;
        let long = session.submit("acter-no-such-command-0123456789a123456789b123456789c123456");
        tokio::time::sleep(Duration::from_secs(5)).await;

        println!("--- what a refused sh session said ---");
        for event in session.events.0.lock().expect("recorder poisoned").iter() {
            println!("  {event:?}");
        }
        println!("--- short heading: {:?} ---", session.heading_of(short));
        println!("--- long heading:  {:?} ---", session.heading_of(long));

        assert_eq!(
            session.heading_of(short),
            Some(Some("lsa".to_owned())),
            "a refused session still heads a command with the line that was submitted"
        );
        assert_eq!(
            session.heading_of(long),
            Some(Some(
                "acter-no-such-command-0123456789a123456789b123456789c123456".to_owned()
            )),
            "and heads a wrapped one in full, where the set-up session truncates it"
        );
        assert!(
            session.rendered().contains("not found"),
            "and still reads the shell's own message aloud: {:?}",
            session.rendered()
        );
        assert!(
            !session.rendered().contains("# lsa"),
            "the echo of the user's own line is not read back to them: {:?}",
            session.rendered()
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs a WSL distribution installed"]
    async fn a_command_that_does_not_exist_still_says_so_in_bash() {
        if !wsl_is_available() {
            println!("skipped: this machine has no WSL distribution");
            return;
        }

        let session = RealSession::wsl().await;
        let command = session.submit("acter-no-such-command");
        tokio::time::sleep(Duration::from_secs(5)).await;

        println!("--- what bash said about a command that does not exist ---");
        for event in session.events.0.lock().expect("recorder poisoned").iter() {
            println!("  {event:?}");
        }
        println!("--- heading: {:?} ---", session.heading_of(command));

        assert!(
            session.rendered().contains("not found"),
            "a listener is told the command does not exist: {:?}",
            session.rendered()
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real shell and needs a WSL distribution installed"]
    async fn setting_a_session_up_writes_nothing_into_the_distribution() {
        if !wsl_is_available() {
            println!("skipped: this machine has no WSL distribution");
            return;
        }

        let session = RealSession::wsl().await;
        let command = session.submit("ls -a ~ | md5sum; echo acter-hashed");
        let output = session.wsl_until(command, "acter-hashed").await;

        let untouched = std::process::Command::new(WSL)
            .args(["--", "sh", "-c", "ls -a ~ | md5sum"])
            .output()
            .expect("wsl answers");
        let expected = String::from_utf8_lossy(&untouched.stdout)
            .split_whitespace()
            .next()
            .expect("md5sum prints a hash")
            .to_owned();

        assert!(
            output.contains(&expected),
            "the home directory an Acter session sees is the one a plain wsl.exe sees: \
             {output:?} does not contain {expected:?}"
        );
    }
}
