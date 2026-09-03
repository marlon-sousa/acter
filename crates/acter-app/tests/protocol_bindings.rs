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
    AttemptId, CommandId, ConnectAnswer, ConnectQuestion, ConnectStep, Connectable, Connected,
    ConnectionKind, ConnectionState, ExitCode, Key, KeyAck, KeyPress, LineId, LineOwner,
    LineRevision, MenuAction, Mode, ProfileId, SessionEvent, SessionId, SetUp, SubmitAck, Variant,
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
    // references yet (SessionId, Mode) and types whose frontend consumer is still to
    // come (KeyPress and KeyAck, which A3.2 will send and read), so the full protocol is
    // emitted before its producers land. Referenced types (CommandId, ExitCode,
    // ConnectionState, Key) come along automatically but are listed for clarity.
    //
    // `ReadMode` is deliberately absent since A6: the autoread verdict is domain-internal
    // and no protocol type references it, so it must not appear in the bindings.
    let types = Types::default()
        .register::<SessionEvent>()
        .register::<SubmitAck>()
        .register::<KeyPress>()
        .register::<Key>()
        .register::<KeyAck>()
        // 28's far-end-line surface. `LineOwner` is an argument to `set_line_owner` and
        // nothing else references it; `LineId` and `LineRevision` come along through
        // `SessionEvent::Output` and are listed for the same reason every other referenced
        // type is — the whole protocol is emitted here, not only what happens to be reached.
        .register::<LineOwner>()
        .register::<LineId>()
        .register::<LineRevision>()
        .register::<SessionId>()
        .register::<CommandId>()
        .register::<ExitCode>()
        .register::<Mode>()
        .register::<ConnectionState>()
        // B7's connect surface. `ProfileId` and `ConnectionKind` come along through
        // `Connectable`, and are listed for the same reason the others are: the whole
        // protocol is emitted here, not only the parts something happens to reference.
        .register::<Connectable>()
        .register::<Variant>()
        .register::<Connected>()
        .register::<ProfileId>()
        .register::<ConnectionKind>()
        // B9's conversation. Connecting stopped being one call and one answer: it reports
        // steps while it happens, asks questions partway, and is answered by invokes of its
        // own. `Secret` comes along through `ConnectAnswer` and is deliberately never
        // exported as something that can be *sent* — it has no `Serialize` at all.
        .register::<ConnectStep>()
        .register::<ConnectQuestion>()
        .register::<ConnectAnswer>()
        .register::<AttemptId>()
        // B9.5's checkbox. It is an argument to `use_profile` rather than something a step or
        // an event carries, so nothing else here references it — which is exactly why it is
        // registered by hand: the whole protocol is emitted here, not only the parts something
        // happens to reference.
        .register::<SetUp>()
        // M3's menu. It is the payload of an emitted event rather than the answer to an
        // invoke, and it is registered for the reason `SetUp` is: the frontend's switch over
        // it must be exhaustive, so a menu item added with no dialog behind it fails to
        // compile instead of reaching a listener as an item that does nothing.
        .register::<MenuAction>();

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
