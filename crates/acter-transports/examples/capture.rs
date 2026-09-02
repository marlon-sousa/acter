//! Experiment rig (not product code): spawn a program on a real pseudoconsole, drive it
//! with a scripted key sequence, and write down both what it sent and what Acter's own
//! terminal engine made of it.
//!
//! It exists to answer the questions roadmap entries 28, 29 and 30 keep asking and
//! nobody can answer from a document: does this program take the alternate screen, what
//! does it send when an arrow key arrives, and how many rows does the engine report as
//! having changed.
//!
//! It runs the real [`AlacrittyEngine`] in the loop, and writes its `take_replies` back
//! to the pseudoconsole — without that a program that asks for a cursor position report
//! (`ESC[6n`) waits forever, which is exactly what the first run of this rig measured.
//!
//! Usage:
//!   cargo run -p acter-transports --example capture -- <out.bin> <script> <program> [args...]
//!
//! Script steps, comma separated and applied in order:
//!   w<ms>      wait that many milliseconds, reading and answering all the while
//!   k:<name>   send a named key (up down left right tab enter esc ctrl_c ctrl_d ctrl_u)
//!   t:<text>   type literal text

use std::io::Write;
use std::time::{Duration, Instant};

use acter_core::{Screen, TerminalEngine, TerminalItem, Transport};
use acter_term::AlacrittyEngine;
use acter_transports::LocalPty;
use tokio::sync::mpsc::{Receiver, channel};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: capture <out.bin> <script> <program> [args...]");
        std::process::exit(2);
    }
    let out = args[0].clone();
    let script = args[1].clone();
    let program = args[2].clone();
    let extra: Vec<&str> = args[3..].iter().map(String::as_str).collect();

    let (columns, lines) = (100u16, 30u16);
    let mut pty = LocalPty::spawn(&program, &extra, &[], columns, lines)
        .unwrap_or_else(|why| panic!("could not spawn {program}: {why}"));
    let (sender, mut receiver) = channel::<Vec<u8>>(256);
    pty.start(sender);

    let mut engine = AlacrittyEngine::new(columns, lines);
    let mut raw = Vec::<u8>::new();

    for step in script.split(',').filter(|step| !step.is_empty()) {
        if let Some(ms) = step.strip_prefix('w') {
            let ms: u64 = ms.parse().expect("milliseconds");
            drain(&mut receiver, &mut engine, &mut pty, &mut raw, ms).await;
        } else if let Some(name) = step.strip_prefix("k:") {
            let bytes = key_bytes(name).unwrap_or_else(|| panic!("unknown key {name}"));
            println!("\n===== KEY {name} =====");
            pty.write(bytes).expect("write key");
        } else if let Some(text) = step.strip_prefix("t:") {
            println!("\n===== TYPE {text} =====");
            pty.write(text.as_bytes()).expect("write text");
        } else {
            panic!("unknown step {step}");
        }
    }

    std::fs::File::create(&out)
        .expect("create output")
        .write_all(&raw)
        .expect("write output");

    println!("\n===== SUMMARY =====");
    println!("raw bytes: {} (written to {out})", raw.len());
    for probe in [
        ("alt screen 1049", &b"\x1b[?1049"[..]),
        ("alt screen 47", &b"\x1b[?47"[..]),
        ("alt screen 1047", &b"\x1b[?1047"[..]),
        ("bracketed paste 2004", &b"\x1b[?2004"[..]),
        ("application cursor keys DECCKM", &b"\x1b[?1h"[..]),
        ("cursor hide 25l", &b"\x1b[?25l"[..]),
        ("cursor position request 6n", &b"\x1b[6n"[..]),
    ] {
        let (name, needle) = probe;
        println!("{name}: {}", contains(&raw, needle));
    }
}

/// Reads for `ms`, feeding every byte to the engine, printing what it reported, and
/// writing the engine's replies back — a device query nobody answers hangs the program.
async fn drain(
    receiver: &mut Receiver<Vec<u8>>,
    engine: &mut AlacrittyEngine,
    pty: &mut LocalPty,
    raw: &mut Vec<u8>,
    ms: u64,
) {
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        match tokio::time::timeout(left, receiver.recv()).await {
            Err(_) => return,
            Ok(None) => return,
            Ok(Some(bytes)) => {
                raw.extend_from_slice(&bytes);
                for item in engine.advance(&bytes) {
                    println!("{}", describe(&item));
                }
                let replies = engine.take_replies();
                if !replies.is_empty() {
                    pty.write(&replies).expect("write replies");
                }
            }
        }
    }
}

fn describe(item: &TerminalItem) -> String {
    match item {
        TerminalItem::Line { id, text, revision } => {
            format!("line {:>4} {:?} {:?}", id.0, revision, text)
        }
        TerminalItem::Marker(marker) => format!("marker {marker:?}"),
        TerminalItem::ScreenChanged(screen) => match screen {
            Screen::Normal => "SCREEN -> normal".to_owned(),
            Screen::Alternate => "SCREEN -> ALTERNATE".to_owned(),
        },
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// The normal (non-application) encodings, which the measurement of 2026-08-31 found
/// both shells accept.
fn key_bytes(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "tab" => b"\t",
        "enter" => b"\r",
        "esc" => b"\x1b",
        "ctrl_c" => b"\x03",
        "ctrl_d" => b"\x04",
        "ctrl_u" => b"\x15",
        "ctrl_x" => b"\x18",
        _ => return None,
    })
}
