//! Experiment rig (not product code): how long a real far end takes to answer one
//! keystroke, measured from the byte leaving Acter to the engine reporting the row.
//!
//! It exists for roadmap 28.1. NVDA answers an arrow key by sending it on, polling the
//! caret for up to `caretMoveTimeoutMs` (default 100 ms, source/editableText.py), and
//! speaking whatever is at the caret when that poll ends. So the only number that
//! decides whether the far-end line can be spoken on the press is this one: the time
//! from the key going out to the redrawn row being in hand. Acter's 500 ms quiescence
//! clock is five times NVDA's window, which is why the listener hears the previous press.
//!
//! For each measured key it prints the time to the first engine item and to the last one
//! in the window -- first change is when a caret could move, last change is when the row
//! has settled and stopped being rewritten.
//!
//! Usage:
//!   cargo run -p acter-transports --example latency -- <window_ms> <script> <program> [args...]
//!
//! Script steps, comma separated:
//!   w<ms>      wait, reading and answering all the while
//!   t:<text>   type literal text
//!   m:<name>   send a named key and measure the answer

use std::time::{Duration, Instant};

use acter_core::{TerminalEngine, TerminalItem, Transport};
use acter_term::AlacrittyEngine;
use acter_transports::LocalPty;
use tokio::sync::mpsc::{Receiver, channel};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: latency <window_ms> <script> <program> [args...]");
        std::process::exit(2);
    }
    let window: u64 = args[0].parse().expect("window milliseconds");
    let script = args[1].clone();
    let program = args[2].clone();
    let extra: Vec<&str> = args[3..].iter().map(String::as_str).collect();

    let (columns, lines) = (100u16, 30u16);
    let mut pty = LocalPty::spawn(&program, &extra, &[], columns, lines)
        .unwrap_or_else(|why| panic!("could not spawn {program}: {why}"));
    let (sender, mut receiver) = channel::<Vec<u8>>(256);
    pty.start(sender);

    let mut engine = AlacrittyEngine::new(columns, lines);
    let mut measurements: Vec<(String, Option<u128>, Option<u128>, usize)> = Vec::new();

    for step in script.split(',').filter(|step| !step.is_empty()) {
        if let Some(ms) = step.strip_prefix('w') {
            let ms: u64 = ms.parse().expect("milliseconds");
            quiet_drain(&mut receiver, &mut engine, &mut pty, ms).await;
        } else if let Some(text) = step.strip_prefix("t:") {
            pty.write(text.as_bytes()).expect("write text");
            quiet_drain(&mut receiver, &mut engine, &mut pty, 400).await;
        } else if let Some(name) = step.strip_prefix("m:") {
            let bytes = key_bytes(name).unwrap_or_else(|| panic!("unknown key {name}"));
            let start = Instant::now();
            pty.write(bytes).expect("write key");
            let (first, last, count) =
                measure(&mut receiver, &mut engine, &mut pty, window, start).await;
            println!("--- {name}: first {first:?} ms, last {last:?} ms, {count} items");
            measurements.push((name.to_owned(), first, last, count));
        } else {
            panic!("unknown step {step}");
        }
    }

    println!("\n===== SUMMARY (milliseconds from the key going out) =====");
    println!("key            first change   settled   items");
    for (name, first, last, count) in &measurements {
        println!(
            "{name:<14} {:>12} {:>9} {:>7}",
            first.map(|ms| ms.to_string()).unwrap_or("-".to_owned()),
            last.map(|ms| ms.to_string()).unwrap_or("-".to_owned()),
            count
        );
    }
    let settled: Vec<u128> = measurements.iter().filter_map(|m| m.2).collect();
    if !settled.is_empty() {
        let worst = settled.iter().max().expect("some");
        println!(
            "\nworst settle: {worst} ms. NVDA's default caret poll is 100 ms (0-2000, settable).",
        );
    }
}

/// Reads for the whole window, timestamping every item the engine reports.
async fn measure(
    receiver: &mut Receiver<Vec<u8>>,
    engine: &mut AlacrittyEngine,
    pty: &mut LocalPty,
    ms: u64,
    start: Instant,
) -> (Option<u128>, Option<u128>, usize) {
    let deadline = start + Duration::from_millis(ms);
    let (mut first, mut last, mut count) = (None, None, 0usize);
    let mut was = engine.cursor();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return (first, last, count);
        }
        match tokio::time::timeout(left, receiver.recv()).await {
            Err(_) => return (first, last, count),
            Ok(None) => return (first, last, count),
            Ok(Some(bytes)) => {
                let items = engine.advance(&bytes);
                let replies = engine.take_replies();
                if !replies.is_empty() {
                    pty.write(&replies).expect("write replies");
                }
                let mut changed = 0usize;
                for item in &items {
                    if let TerminalItem::Line { id, text, revision } = item {
                        changed += 1;
                        println!(
                            "      +{:>4} ms  line {:>3} {:?} {:?}",
                            start.elapsed().as_millis(),
                            id.0,
                            revision,
                            text
                        );
                    }
                }
                // A bare cursor move -- left, right, Home -- rewrites no line, so the
                // cursor is the whole of the evidence that the key arrived.
                let now = engine.cursor();
                if (now.column, now.row) != (was.column, was.row) {
                    println!(
                        "      +{:>4} ms  cursor {},{} -> {},{}",
                        start.elapsed().as_millis(),
                        was.row,
                        was.column,
                        now.row,
                        now.column
                    );
                    was = now;
                    if changed == 0 {
                        changed = 1;
                    }
                }
                if changed > 0 {
                    let at = start.elapsed().as_millis();
                    first.get_or_insert(at);
                    last = Some(at);
                    count += changed;
                }
            }
        }
    }
}

async fn quiet_drain(
    receiver: &mut Receiver<Vec<u8>>,
    engine: &mut AlacrittyEngine,
    pty: &mut LocalPty,
    ms: u64,
) {
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        match tokio::time::timeout(left, receiver.recv()).await {
            Err(_) | Ok(None) => return,
            Ok(Some(bytes)) => {
                let _ = engine.advance(&bytes);
                let replies = engine.take_replies();
                if !replies.is_empty() {
                    pty.write(&replies).expect("write replies");
                }
            }
        }
    }
}

fn key_bytes(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "home" => b"\x1b[H",
        "end" => b"\x1b[F",
        "tab" => b"\t",
        "backspace" => b"\x7f",
        "ctrl_c" => b"\x03",
        _ => return None,
    })
}
