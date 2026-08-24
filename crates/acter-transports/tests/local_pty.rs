//! Integration test: `LocalPty` against a real shell on a real pseudoconsole.
//!
//! **Every test here is `#[ignore]`d**, so `cargo test --workspace` spawns no process and
//! stays independent of what happens to be installed on the machine. CI runs them
//! explicitly in the `real-shell (Windows)` job, which is what stops them from rotting
//! unrun (spec B4, decision 10).
//!
//! Run them by hand with:
//! `cargo test -p acter-transports --test local_pty -- --ignored --nocapture`
//!
//! `--nocapture` matters for one of them: [`read_timing`] is a *measurement* rather than
//! an assertion, and its whole output is the point (spec B4, decision 6).
//!
//! **The harness answers device queries, because a real shell will not start without
//! one.** The first thing `cmd.exe` puts on the wire is `ESC [ 6 n` — a cursor-position
//! report request — and it does not draw its prompt until something answers. So these
//! tests run the reads through a real [`AlacrittyEngine`] and write its
//! `take_replies()` back, which is precisely what `SessionService` does in production.
//! That was found by writing these tests without it and watching every one of them time
//! out having seen four bytes; B3 decision 4 built the reply path on reasoning about
//! programs that wait forever, and this is the first evidence that a *shell* is one of
//! them, before it has said anything at all.

use std::time::{Duration, Instant};

use acter_core::{TerminalEngine, TerminalItem, Transport, TransportError};
use acter_term::AlacrittyEngine;
use acter_transports::LocalPty;
use tokio::sync::mpsc::{Receiver, channel};

/// The shell these tests drive. `cmd.exe` rather than PowerShell deliberately: it starts
/// in tens of milliseconds instead of seconds, it is present on every Windows machine
/// including the CI image, and nothing here is about shell behavior — what is under test
/// is the pipe, and B5 is where a particular shell starts mattering.
const SHELL: &str = "cmd.exe";

/// Long enough that a slow CI machine is not what fails a test, short enough that a
/// genuine hang is reported as one rather than sitting there.
const PATIENCE: Duration = Duration::from_secs(20);

/// What `SessionService` writes when the user presses Enter, restated here so this suite
/// submits exactly what the product submits.
const ENTER: char = '\r';

/// `PacingConfig::quiescence`'s default, restated here because this file measures against
/// it: silence this long is what turns accumulated output into a chunk, and therefore what
/// decides whether a gap in the byte stream is audible at all.
const QUIESCENCE: Duration = Duration::from_millis(500);

/// A started session, with the reads it has produced so far.
struct Shell {
    pty: LocalPty,
    reads: Receiver<Vec<u8>>,
    /// Only ever asked for its device-query replies. What the *text* means is asserted
    /// through the engine in `pipeline.rs`; here the engine is the thing that keeps a
    /// real shell talking.
    engine: AlacrittyEngine,
}

impl Shell {
    /// Started with the arguments the *application* starts this shell with, which since
    /// B5.1 means asking the same adapter the composition root asks instead of spelling
    /// them out here. That is the point of the entry: `/Q` (cmd's own command echo off)
    /// and `/K` (keep running after each command) used to live in this file and nowhere
    /// else, so every measurement here was taken against a stream the product never
    /// produced (spec B5.1, decision 5).
    ///
    /// **The injection is deliberately not taken with them.** What this suite is about is
    /// the pipe, and its subject is an unmarked stream; the environment half of the launch
    /// is proved to reach a real shell in `real_session.rs`, where a marker means
    /// something.
    fn start() -> Self {
        let launch = acter_shells::adapter_for(SHELL).launch();
        let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
        Self::with(&args, &[])
    }

    /// A shell started with arguments this suite chose rather than the adapter's — `/C`,
    /// which runs one command and exits, is not how Acter starts a session and is only
    /// ever what a test about a far end *going away* needs.
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

    /// Submits the way `SessionService` does: the line, then a carriage return.
    ///
    /// Deliberately the same bytes as the domain writes, so this suite exercises the
    /// shipped terminator. It is not a detail — see
    /// [`a_line_feed_is_not_enter_and_a_carriage_return_is`].
    fn submit(&mut self, line: &str) {
        self.pty
            .write(format!("{line}{ENTER}").as_bytes())
            .expect("the line reaches the shell");
    }

    /// Reads until `wanted` has been seen, and returns everything read up to then.
    ///
    /// Panics with what it *did* see, because a timeout whose message is "timed out" makes
    /// a real failure and a flaky machine look identical.
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

    /// Answers whatever the read asked for, the way the pump does: an emulator does not
    /// answer a device query itself, and a shell that asked one waits forever.
    ///
    /// Returns what the read meant, which most callers here ignore — this suite asserts on
    /// text rather than on items. [`a_shell_that_exits_ends_the_session_by_closing_the_channel`]
    /// is the exception, and it needs the items the engine was computing and dropping
    /// anyway (spec B4.3, decision 2).
    fn answer(&mut self, read: &[u8]) -> Vec<TerminalItem> {
        let items = self.engine.advance(read);
        let replies = self.engine.take_replies();
        if !replies.is_empty() {
            let _ = self.pty.write(&replies);
        }
        items
    }
}

/// The first end-to-end fact: a real shell starts, says something unprompted, and its
/// bytes arrive as reads.
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

/// The echo, produced by something nobody scripted.
///
/// This is the text B6.1 reads a block's heading from, and until now every byte of it
/// came from a transcript that was written to produce it. A pseudoconsole echoes what was
/// written to it because that is what a console does.
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

/// The interrupt **reaches** the far end as a control byte in the data stream, and the
/// shell goes on working afterwards (spec B4, decision 4).
///
/// Named for exactly what it proves and no more. `pause` ends on *any* keypress, so this
/// says the byte arrived and the session survived it — **not** that a running program was
/// stopped. That stronger claim is [`an_interrupt_stops_a_running_program`], and
/// "survives" against a program that had to be *signalled* is
/// [`a_session_goes_on_working_after_a_real_program_was_stopped`].
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

/// A resize reaches the pseudoconsole, and the shell sees the new width.
///
/// `mode con` reports the console's own dimensions, so this asserts through the shell
/// rather than through the API that was just called — the API answering its own question
/// would prove nothing about what the far end was told.
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

/// The session ends by the channel closing, not by an error (spec B4, decision 3) — and
/// what a pseudoconsole says on its way down means nothing to a user (spec B4.3).
///
/// **It drains rather than asserting on the next read, and that is the point.** ConPTY
/// restores the modes it set as it tears the pseudoconsole down, writing `ESC[?9001l
/// ESC[?1004l` — win32-input-mode and focus-event reporting back off. Whether those bytes
/// beat the close is a race this machine loses at roughly even odds and CI wins every time,
/// so "the next thing is the close" was asserting a coin flip. How many reads an ending
/// takes is not this suite's business; that it *ends* is.
///
/// **What is asserted instead is the half that matters to a user.** Everything arriving
/// after the last output goes through the engine, and none of it may mean anything —
/// measured 2026-08-23 across fourteen runs that saw the epilogue, which produced no
/// `TerminalItem` at all. An escape sequence read aloud at the end of every session would
/// be a real defect, and this is what would catch it.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_shell_that_exits_ends_the_session_by_closing_the_channel() {
    // /C runs one command and exits, which is a shell ending on its own rather than one
    // that was killed.
    let mut shell = Shell::over(&["/C", "echo done"]);
    shell.until("done").await;

    let mut epilogue: Vec<u8> = Vec::new();
    let mut items: Vec<TerminalItem> = Vec::new();
    loop {
        let read = tokio::time::timeout(PATIENCE, shell.reads.recv())
            .await
            .expect("the session ends within the patience window");
        // `None` is the ending, and the only one the port has.
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

/// Writing to a shell that has gone answers the sentence that says the session ended.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn writing_to_a_shell_that_exited_says_the_session_ended() {
    let mut shell = Shell::over(&["/C", "echo done"]);
    shell.until("done").await;
    while let Some(read) = shell.reads.recv().await {
        shell.answer(&read);
    }

    // Repeated because the failure surfaces only once the operating system has actually
    // torn the pipe down, which is not synchronous with the process exiting. What is
    // asserted is that it never becomes a *different* answer.
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
            // Windows can keep a pseudoconsole's write side accepting bytes after the
            // attached process has gone. Not a failure of this adapter: the session has
            // already ended the way the port models it, by the read channel closing.
            println!("note: the write side still accepted bytes after the shell exited");
        }
        Err(error) => assert_eq!(error, TransportError::Closed),
    }
}

/// **The measurement, not an assertion** (spec B4, decision 6).
///
/// Prints the gaps between reads while a real shell produces sustained output, so the
/// question the roadmap filed here — whether the fake pipe needs to model the gap between
/// the reads of one delivery — is answered against numbers somebody took rather than a
/// pattern somebody guessed. What matters is how the gaps compare with
/// `PacingConfig::quiescence` (500 ms): a gap that size *inside* one line is what would
/// make speech read half a sentence.
///
/// It also counts the case the *policy* question turns on: a gap at or over the
/// quiescence window arriving while the text so far ends **mid-line**. That is the one
/// that would make the auto-read policy flush half a sentence, and until now nobody knew
/// whether a real shell produces it at all.
///
/// The output belongs in the PR body.
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
        // A fixed window rather than "until it finishes": what is being characterised is
        // the rhythm of a stream, and the first seconds of one are as representative as
        // its last.
        while started.elapsed() < Duration::from_secs(5) {
            let Ok(Some(read)) =
                tokio::time::timeout(Duration::from_secs(2), shell.reads.recv()).await
            else {
                break;
            };
            let gap = last.elapsed();
            // A gap this long, with a line still unfinished on the other side of it, is
            // the shape that would have speech read half a sentence.
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

/// One measured stream, said in numbers a decision can be made from.
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

/// **The defect a real shell found, pinned.**
///
/// The domain used to write a bare line feed. A pseudoconsole echoes it and the shell
/// never runs the line — it is still waiting for an Enter that never came — so every
/// command in a real session was accepted and then silently did nothing. It was invisible
/// against the scripted far end, whose line discipline takes either byte as a line
/// ending, and it is what a manual NVDA session against `cmd.exe` surfaced: the echo
/// reached the buffer and no output ever followed.
///
/// Both halves are asserted, because the interesting one is the negative: a line feed
/// alone is echoed and *not run*.
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

        // The echo comes back either way. What says the shell *ran* the line is the word
        // appearing a second time, on an output line of its own.
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

/// **The requirement B4.1 closed**: an interrupt stops a program that is genuinely
/// running, and what makes it work is in `LocalPty::spawn` rather than in the byte.
///
/// It was invisible until a real shell ran a real program: `pause` ends on any keypress,
/// so the test beside this one passes whether or not an interrupt means anything.
///
/// **This test is only as good as the state the shell was spawned with**, which is the
/// trap that cost B4.1 four disproven mechanisms. A shell that inherited an "ignore
/// Ctrl+C" attribute refuses the signal however it is encoded, so before the fix this
/// failed under any launcher that groups its children, and the failure was silent and
/// total. It should now pass however this suite was launched — that is precisely what
/// `LocalPty::spawn` clearing the attribute buys, and therefore a failure here is a real
/// failure rather than an artifact of the harness.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn an_interrupt_stops_a_running_program() {
    let mut shell = Shell::start();
    shell.until(">").await;

    shell.submit("ping -n 20 127.0.0.1");
    shell.until("Reply from").await;

    shell.pty.interrupt().expect("the interrupt is delivered");

    // Anything arriving from here on is the program still going.
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

/// The session survives an interrupt that had to *signal* something, not merely one a
/// program was waiting for.
///
/// Its sibling above drives `pause`, which ends on any keypress; this one stops a real
/// program and then submits another command, so what is proven is that a pseudoconsole
/// whose program was signalled is still a working session (spec B4.1).
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

/// **The requirement B4.6 measured**: an interrupt survives a shell Acter did not spawn
/// and cannot reach — `docker run -it`, and by the same mechanism `wsl`, `ssh`,
/// `kubectl exec`.
///
/// B4.1's mechanism cannot be what carries it. That one is the transitive inheritance of
/// a Windows console attribute, Acter to shell to program, and a container's shell is not
/// in this process tree at all: it is the daemon's child, in another kernel, behind a
/// client. What carries the interrupt here is the *byte*, travelling as data through
/// `docker.exe` to a tty the container's own line discipline is watching — a second,
/// independent mechanism that gives Acter the same behaviour for a completely different
/// reason.
///
/// **It asserts against the container rather than against our own stream, and that is the
/// whole design of the test.** The entry named two outcomes that look identical from
/// here, because output stops either way: the byte passing through as data and the
/// program inside being signalled, or ConPTY turning it into a console control event that
/// kills the *client* and orphans the container. So the question is put to the container:
/// its own stdout must stop advancing, and the session must still be talking to it
/// afterwards.
///
/// **What it guards against is a plausible future change**, which is why it earns its
/// runtime: replacing the byte written by `Transport::interrupt` with a Windows console
/// control API would leave every test beside this one passing and silently take the
/// interrupt away from every proxied session.
///
/// Skipped rather than failed on a machine without Docker, for `real_session.rs`'s reason:
/// a machine without Docker has not discovered a defect.
#[tokio::test]
#[ignore = "spawns a real shell and a container"]
async fn an_interrupt_survives_a_proxied_shell() {
    if !docker_is_available() || !image_is_present() {
        println!("skipped: Docker is not available on this machine");
        return;
    }

    // Deliberately not `--rm`: `docker logs` and `docker inspect` after the interrupt are
    // the measurement, and `--rm` would delete the evidence in the outcome that matters.
    // The container is removed by `Container`'s drop instead, including when an assertion
    // below panics — a leftover from a failed run would otherwise read as this run's.
    let container = Container::named("acter-interrupt-through-a-proxy");
    let mut shell = Shell::start();
    shell.until(">").await;

    // Waiting for the container's own prompt rather than for an echo of our own is what
    // keeps the outer `cmd.exe` from answering by mistake: until `sh` has drawn `/ #`, a
    // line submitted here could still be read by the shell we came from.
    shell.submit(&format!(
        "docker run -it --name {} {IMAGE} sh",
        container.name
    ));
    shell.until("/ #").await;

    // One tick per second, so the container's own stdout is a clock: a loop that survived
    // the interrupt says so by having counted further than we ever saw.
    shell.submit("i=1; while true; do echo tick-$i; i=$((i+1)); sleep 1; done");
    let ticking = shell.until("tick-3").await;

    shell.pty.interrupt().expect("the interrupt is delivered");

    // Long enough that a surviving loop would have ticked several more times.
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

    // One tick of slack, because the interrupt can land in the moment between the loop
    // printing and the byte arriving, and that tick is on its way either way.
    let before = last_tick(&ticking).expect("the loop ticked before the stop");
    let ours = last_tick(&after).unwrap_or(before).max(before);
    assert!(
        ours <= before + 1,
        "the loop went on ticking after the interrupt: our stream reached tick-{ours}, \
         against tick-{before} when the interrupt was sent"
    );

    // The half our own stream cannot answer: it stopped, but a client killed by a console
    // control event would look exactly the same from here while the loop ran on unheard.
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

    // And the session is still talking to the container rather than to the shell we came
    // from: `uname` answers in `sh` and is an error in `cmd.exe`, so this fails loudly in
    // the outcome where the client died and left us at the outer prompt.
    shell.submit("uname -s");
    let who = shell.until("Linux").await;
    assert!(
        who.contains("Linux"),
        "the container's shell is still the far end: {who:?}"
    );
}

/// The container image the proxied case runs in: tiny, and its `sh` is as plain as a shell
/// gets. `real_session.rs` runs the same one for the same reason.
const IMAGE: &str = "alpine";

/// A container that is removed when the test ends, however it ends.
///
/// **Not a nicety.** Measured 2026-08-23: a container outlives the session that started
/// it, `--rm` notwithstanding, because `LocalPty::drop` kills `docker.exe` and the
/// container is the daemon's child rather than the client's — which is the same fact this
/// test's interrupt depends on, seen from the other side. Closing a real `cmd.exe` window
/// leaves it running too, so it is not Acter's to fix (spec B4.6); it *is* this test's to
/// clean up after.
struct Container {
    name: String,
}

impl Container {
    fn named(name: &str) -> Self {
        // A leftover from a killed run would read as this run's container.
        let _ = docker(&["rm", "-f", name]);
        Self {
            name: name.to_owned(),
        }
    }

    /// Everything the container has written to its own stdout, which is the observer that
    /// is not our stream.
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

/// The highest `tick-N` in some text.
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

/// Whether this machine can run the proxied case at all. Deliberately a check on the
/// daemon rather than on the client: `docker` being on `PATH` with nothing behind it is
/// the common shape of "installed", and it would hang the test rather than skip it.
fn docker_is_available() -> bool {
    docker(&["info", "--format", "{{.ServerVersion}}"])
}

/// Pulls the image before the session starts, so a cold machine waits here rather than
/// inside the pseudoconsole, where a pull's carriage-return progress bars would become
/// part of what the test is reading.
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
