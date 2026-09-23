//! Integration test: `LocalPty` against a real shell on a real pseudoconsole.

use std::time::{Duration, Instant};

use acter_core::{TerminalEngine, TerminalItem, Transport, TransportError};
use acter_term::AlacrittyEngine;
use acter_transports::LocalPty;
use tokio::sync::mpsc::{Receiver, channel};

const SHELL: &str = "cmd.exe";

const PATIENCE: Duration = Duration::from_secs(20);

const ENTER: char = '\r';

/// Must equal `PacingConfig::quiescence`'s default.
const QUIESCENCE: Duration = Duration::from_millis(500);

struct Shell {
    pty: LocalPty,
    reads: Receiver<Vec<u8>>,
    engine: AlacrittyEngine,
}

impl Shell {
    fn start() -> Self {
        let launch = acter_shells::adapter_for(SHELL).launch();
        let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
        Self::with(&args, &[])
    }

    fn over(args: &[&str]) -> Self {
        Self::with(args, &[])
    }

    fn with(args: &[&str], environment: &[(&str, &str)]) -> Self {
        let mut pty = LocalPty::spawn(SHELL, args, environment, 80, 24).expect("a shell starts");
        let (bytes, reads) = channel(1024);
        pty.start(bytes);
        Self {
            pty,
            reads,
            engine: AlacrittyEngine::new(80, 24),
        }
    }

    fn submit(&mut self, line: &str) {
        self.pty
            .write(format!("{line}{ENTER}").as_bytes())
            .expect("the line reaches the shell");
    }

    async fn until(&mut self, wanted: &str) -> String {
        let mut seen = String::new();
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            let Ok(Some(read)) = tokio::time::timeout(PATIENCE, self.reads.recv()).await else {
                break;
            };
            self.answer(&read);
            seen.push_str(&String::from_utf8_lossy(&read));
            if seen.contains(wanted) {
                return seen;
            }
        }
        panic!("never saw {wanted:?}. What the shell said was:\n{seen}");
    }

    /// `cmd.exe` sends `ESC[6n` before its prompt and draws nothing until it is answered.
    fn answer(&mut self, read: &[u8]) -> Vec<TerminalItem> {
        let items = self.engine.advance(read);
        let replies = self.engine.take_replies();
        if !replies.is_empty() {
            let _ = self.pty.write(&replies);
        }
        items
    }
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_real_shell_starts_and_says_something() {
    let mut shell = Shell::start();

    shell.submit("echo acter-is-here");

    let seen = shell.until("acter-is-here").await;
    assert!(
        seen.contains("acter-is-here"),
        "the shell answered: {seen:?}"
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_submitted_line_comes_back_echoed() {
    let mut shell = Shell::start();

    shell.submit("echo the-echo-is-real");

    let seen = shell.until("the-echo-is-real").await;
    assert!(
        seen.matches("the-echo-is-real").count() >= 2,
        "the line was echoed as it was typed and again as output: {seen:?}"
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn an_interrupt_reaches_the_shell_and_the_session_survives_it() {
    let mut shell = Shell::start();

    shell.submit("pause");
    shell.until("any key").await;

    shell.pty.interrupt().expect("the interrupt is delivered");

    shell.submit("echo alive-after-the-stop");
    let seen = shell.until("alive-after-the-stop").await;
    assert!(seen.contains("alive-after-the-stop"));
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_resize_reaches_the_far_end() {
    let mut shell = Shell::start();
    shell.until(">").await;

    shell.pty.resize(100, 30).expect("the resize is accepted");

    shell.submit("mode con");
    let seen = shell.until("Columns").await;
    assert!(
        seen.contains("100"),
        "the shell reports the width it was resized to: {seen}"
    );
}

/// ConPTY writes `ESC[?9001l ESC[?1004l` while tearing down, sometimes before the close and
/// sometimes after it, so this drains to the close and asserts none of it becomes an item.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_shell_that_exits_ends_the_session_by_closing_the_channel() {
    let mut shell = Shell::over(&["/C", "echo done"]);
    shell.until("done").await;

    let mut epilogue: Vec<u8> = Vec::new();
    let mut items: Vec<TerminalItem> = Vec::new();
    loop {
        let read = tokio::time::timeout(PATIENCE, shell.reads.recv())
            .await
            .expect("the session ends within the patience window");
        // `None` is the session ending.
        let Some(read) = read else { break };
        items.extend(shell.answer(&read));
        epilogue.extend_from_slice(&read);
    }

    assert!(
        items.is_empty(),
        "the shell's teardown said something a user would hear: {items:?}, from {:?}",
        String::from_utf8_lossy(&epilogue)
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn writing_to_a_shell_that_exited_says_the_session_ended() {
    let mut shell = Shell::over(&["/C", "echo done"]);
    shell.until("done").await;
    while let Some(read) = shell.reads.recv().await {
        shell.answer(&read);
    }

    // The write fails only once Windows has torn the pipe down, which lags the process exit.
    let mut last = Ok(());
    for _ in 0..50 {
        last = shell.pty.write(b"echo still there\r\n");
        if last.is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    match last {
        Ok(()) => {
            // Windows can keep a pseudoconsole's write side accepting bytes after the shell exits.
            println!("note: the write side still accepted bytes after the shell exited");
        }
        Err(error) => assert_eq!(error, TransportError::Closed),
    }
}

/// Prints read gaps against the quiescence window rather than asserting; run with `--nocapture`.
#[tokio::test]
#[ignore = "spawns a real shell; prints a measurement"]
async fn read_timing() {
    for (what, command) in [
        ("a large directory listing", "dir /s C:\\Windows\\System32"),
        ("a program that dribbles", "ping -n 4 127.0.0.1"),
    ] {
        let mut shell = Shell::start();
        shell.until(">").await;

        let started = Instant::now();
        shell.submit(command);

        let mut gaps: Vec<Duration> = Vec::new();
        let mut bytes = 0usize;
        let mut straddled = 0usize;
        let mut mid_line = false;
        let mut last = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            let Ok(Some(read)) =
                tokio::time::timeout(Duration::from_secs(2), shell.reads.recv()).await
            else {
                break;
            };
            let gap = last.elapsed();
            if gap >= QUIESCENCE && mid_line {
                straddled += 1;
            }
            gaps.push(gap);
            last = Instant::now();
            bytes += read.len();
            mid_line = !read.ends_with(b"\n") && !read.ends_with(b"\r");
            shell.answer(&read);
        }

        report(what, command, &gaps, bytes, straddled);
    }
}

fn report(what: &str, command: &str, gaps: &[Duration], bytes: usize, straddled: usize) {
    let mut sorted: Vec<u128> = gaps.iter().map(Duration::as_micros).collect();
    sorted.sort_unstable();
    let at = |fraction: f64| -> u128 {
        if sorted.is_empty() {
            return 0;
        }
        let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
        sorted[index]
    };
    let over_quiescence = sorted
        .iter()
        .filter(|gap| **gap >= QUIESCENCE.as_micros())
        .count();

    println!("--- read timing: {what} ({command})");
    println!("    reads: {}, bytes: {bytes}", sorted.len());
    println!(
        "    gap microseconds — median {}, p90 {}, p99 {}, max {}",
        at(0.5),
        at(0.9),
        at(0.99),
        sorted.last().copied().unwrap_or(0)
    );
    println!("    gaps at or over the {QUIESCENCE:?} quiescence window: {over_quiescence}");
    println!("    of those, with a line left unfinished across the gap: {straddled}");
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_line_feed_is_not_enter_and_a_carriage_return_is() {
    for (terminator, runs) in [('\n', false), (ENTER, true)] {
        let mut shell = Shell::start();
        shell.until(">").await;

        shell
            .pty
            .write(format!("echo ran-it{terminator}").as_bytes())
            .expect("the write reaches the shell");

        let mut seen = String::new();
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
            let Ok(Some(read)) =
                tokio::time::timeout(Duration::from_secs(1), shell.reads.recv()).await
            else {
                break;
            };
            shell.answer(&read);
            seen.push_str(&String::from_utf8_lossy(&read));
        }

        let ran = seen.matches("ran-it").count() >= 2;
        assert_eq!(
            ran,
            runs,
            "terminator {terminator:?}: expected the shell to {} — what came back was {seen:?}",
            if runs {
                "run the line"
            } else {
                "echo the line and wait"
            }
        );
    }
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn an_interrupt_stops_a_running_program() {
    let mut shell = Shell::start();
    shell.until(">").await;

    shell.submit("ping -n 20 127.0.0.1");
    shell.until("Reply from").await;

    shell.pty.interrupt().expect("the interrupt is delivered");

    let mut after = String::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        let Ok(Some(read)) = tokio::time::timeout(Duration::from_secs(2), shell.reads.recv()).await
        else {
            break;
        };
        shell.answer(&read);
        after.push_str(&String::from_utf8_lossy(&read));
    }

    assert_eq!(
        after.matches("Reply from").count(),
        0,
        "the program was still running after the interrupt: {after:?}"
    );
}

#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_session_goes_on_working_after_a_real_program_was_stopped() {
    let mut shell = Shell::start();
    shell.until(">").await;

    shell.submit("ping -n 20 127.0.0.1");
    shell.until("Reply from").await;

    shell.pty.interrupt().expect("the interrupt is delivered");

    shell.submit("echo alive-after-a-real-stop");
    let seen = shell.until("alive-after-a-real-stop").await;
    assert!(seen.contains("alive-after-a-real-stop"));
}

#[tokio::test]
#[ignore = "spawns a real shell and a container"]
async fn an_interrupt_survives_a_proxied_shell() {
    if !docker_is_available() || !image_is_present() {
        println!("skipped: Docker is not available on this machine");
        return;
    }

    // Not `--rm`: `docker logs` and `docker inspect` are read after the interrupt.
    let container = Container::named("acter-interrupt-through-a-proxy");
    let mut shell = Shell::start();
    shell.until(">").await;

    // Until `sh` draws `/ #`, a submitted line can still reach the outer `cmd.exe`.
    shell.submit(&format!(
        "docker run -it --name {} {IMAGE} sh",
        container.name
    ));
    shell.until("/ #").await;

    shell.submit("i=1; while true; do echo tick-$i; i=$((i+1)); sleep 1; done");
    let ticking = shell.until("tick-3").await;

    shell.pty.interrupt().expect("the interrupt is delivered");

    let mut after = String::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        let Ok(Some(read)) = tokio::time::timeout(Duration::from_secs(2), shell.reads.recv()).await
        else {
            break;
        };
        shell.answer(&read);
        after.push_str(&String::from_utf8_lossy(&read));
    }

    // One tick of slack: the interrupt can land between the loop printing and the byte arriving.
    let before = last_tick(&ticking).expect("the loop ticked before the stop");
    let ours = last_tick(&after).unwrap_or(before).max(before);
    assert!(
        ours <= before + 1,
        "the loop went on ticking after the interrupt: our stream reached tick-{ours}, \
         against tick-{before} when the interrupt was sent"
    );

    // Our stream stops too if a console control event killed `docker.exe` and orphaned the loop.
    let inside = last_tick(&container.logs()).expect("the container recorded its own ticks");
    assert!(
        inside <= ours,
        "the loop went on ticking inside the container after the interrupt: it reached \
         tick-{inside} and the last one we ever saw was tick-{ours}"
    );
    assert_eq!(
        container.state(),
        "running",
        "the interrupt stopped the program in the container, not the container"
    );

    shell.submit("uname -s");
    let who = shell.until("Linux").await;
    assert!(
        who.contains("Linux"),
        "the container's shell is still the far end: {who:?}"
    );
}

const IMAGE: &str = "alpine";

/// A container outlives the `LocalPty` that started it, `--rm` notwithstanding, because dropping
/// it kills `docker.exe` and the container is the daemon's child.
struct Container {
    name: String,
}

impl Container {
    fn named(name: &str) -> Self {
        let _ = docker(&["rm", "-f", name]);
        Self {
            name: name.to_owned(),
        }
    }

    fn logs(&self) -> String {
        docker_output(&["logs", &self.name])
    }

    fn state(&self) -> String {
        docker_output(&["inspect", "-f", "{{.State.Status}}", &self.name])
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        let _ = docker(&["rm", "-f", &self.name]);
    }
}

fn last_tick(text: &str) -> Option<u32> {
    text.split("tick-")
        .skip(1)
        .filter_map(|rest| {
            rest.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse::<u32>()
                .ok()
        })
        .max()
}

/// Checks the daemon, because a `docker` client with no daemon behind it hangs instead of failing.
fn docker_is_available() -> bool {
    docker(&["info", "--format", "{{.ServerVersion}}"])
}

/// Pulls before the session starts, so a pull's progress bars never reach the pseudoconsole.
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

fn docker_output(args: &[&str]) -> String {
    std::process::Command::new("docker")
        .args(args)
        .output()
        .map(|out| {
            let mut said = String::from_utf8_lossy(&out.stdout).into_owned();
            said.push_str(&String::from_utf8_lossy(&out.stderr));
            said.trim().to_owned()
        })
        .unwrap_or_default()
}
