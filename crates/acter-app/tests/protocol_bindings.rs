//! Generator + drift guard for the frontend's protocol bindings.
//!
//! This test *is* the generator (the canonical specta pattern): it renders the
//! `acter-core` IPC types to TypeScript and writes `ui/src/protocol.ts`, which is
//! committed. CI regenerates and `git diff --exit-code` fails on a stale binding, so
//! the wire contract lives in exactly one place (the Rust types) and cannot drift.
//!
//! Types only: no invoke/channel runtime is emitted (the frontend router hand-writes
//! typed invoke wrappers per ARCHITECTURE.md), so the output imports nothing and
//! `routers/tauri.ts` stays the sole importer of `@tauri-apps/api`.

use std::fs;
use std::path::PathBuf;

use acter_core::{
    CommandId, ConnectionState, ExitCode, Mode, ReadMode, SessionEvent, SessionId, SubmitAck,
};
use specta::Types;
use specta_typescript::Typescript;

const BINDINGS_PATH: &str = "../../ui/src/protocol.ts";

const HEADER: &str = "\
// GENERATED — do not edit by hand.
// Source of truth: acter-core IPC types (crates/acter-core/src/entities/).
// Regenerate: cargo test -p acter-app --test protocol_bindings";

fn render() -> String {
    // Register the whole surface explicitly — including types no event/command
    // references yet (SessionId, Mode), so the full protocol is emitted before its
    // producers land. Referenced types (CommandId, ExitCode, ReadMode,
    // ConnectionState) come along automatically but are listed for clarity.
    let types = Types::default()
        .register::<SessionEvent>()
        .register::<SubmitAck>()
        .register::<SessionId>()
        .register::<CommandId>()
        .register::<ExitCode>()
        .register::<ReadMode>()
        .register::<Mode>()
        .register::<ConnectionState>();

    Typescript::default()
        .header(HEADER)
        .export(&types, specta_serde::Format)
        .expect("protocol types must export to TypeScript")
}

fn bindings_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(BINDINGS_PATH)
}

#[test]
fn protocol_bindings_are_up_to_date() {
    let generated = render();
    // Write with LF exactly as rendered (the file is pinned to LF in .gitattributes),
    // so a Windows checkout and this generator agree byte-for-byte and the CI drift
    // check is deterministic.
    fs::write(bindings_path(), generated.as_bytes())
        .expect("ui/src/protocol.ts must be writable by the generator");
}
