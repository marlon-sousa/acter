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

use acter_core::{TerminalEngine, Transport, TransportError};
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
    fn start() -> Self {
        // /Q turns the echo of the shell's own commands off, so what comes back is the
        // pseudoconsole's echo of what was typed rather than cmd's script-style
        // re-printing of it. /K keeps it running after each command.
        Self::over(&["/Q", "/K"])
    }

    fn over(args: &[&str]) -> Self {
        let mut pty = LocalPty::spawn(SHELL, args, 80, 24).expect("a shell starts");
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
            .write(format!("{line}\r\n").as_bytes())
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
    fn answer(&mut self, read: &[u8]) {
        self.engine.advance(read);
        let replies = self.engine.take_replies();
        if !replies.is_empty() {
            let _ = self.pty.write(&replies);
        }
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

/// The interrupt, as a control byte in the data stream (spec B4, decision 4).
///
/// `pause` waits for a keypress forever; the interrupt is what ends it. The assertion is
/// that the shell comes back to a prompt and answers again — "it stopped" is only
/// observable as "it is listening once more", which is the same thing the domain above
/// concludes from the bytes that follow.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn an_interrupt_stops_a_command_that_would_not_have_ended() {
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

/// The session ends by the channel closing, not by an error (spec B4, decision 3).
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_shell_that_exits_ends_the_session_by_closing_the_channel() {
    // /C runs one command and exits, which is a shell ending on its own rather than one
    // that was killed.
    let mut shell = Shell::over(&["/C", "echo done"]);
    shell.until("done").await;

    let ended = tokio::time::timeout(PATIENCE, shell.reads.recv())
        .await
        .expect("the session ends within the patience window");

    assert!(
        ended.is_none(),
        "the far end going away is the channel closing, never an error: {ended:?}"
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
