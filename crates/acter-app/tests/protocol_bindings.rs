//! Generator and drift guard: writes `ui/src/protocol.ts` from the `acter-core` IPC types, and
//! CI fails on a `git diff` of the result.

use std::fs;
use std::path::PathBuf;

use acter_core::{
    AttemptId, CommandId, ConnectAnswer, ConnectQuestion, ConnectStep, Connectable, Connected,
    ConnectionKind, ConnectionState, ExitCode, Key, KeyAck, KeyPress, LaunchRequest, LineId,
    LineOwner, LineRevision, MenuAction, Mode, ProfileId, SavedConnections, SavedRow, SessionEvent,
    SessionId, SetUp, SubmitAck, Variant,
};
use specta::Types;
use specta_typescript::Typescript;

const BINDINGS_PATH: &str = "../../ui/src/protocol.ts";

const HEADER: &str = "\
// GENERATED — do not edit by hand.
// Source of truth: acter-core IPC types (crates/acter-core/src/entities/).
// Regenerate: cargo test -p acter-app --test protocol_bindings";

fn render() -> String {
    // A type no other registered type references, such as `LineOwner` or `SetUp`, is emitted
    // only because it is registered here.
    let types = Types::default()
        .register::<SessionEvent>()
        .register::<SubmitAck>()
        .register::<KeyPress>()
        .register::<Key>()
        .register::<KeyAck>()
        .register::<LineOwner>()
        .register::<LineId>()
        .register::<LineRevision>()
        .register::<SessionId>()
        .register::<CommandId>()
        .register::<ExitCode>()
        .register::<Mode>()
        .register::<ConnectionState>()
        .register::<Connectable>()
        .register::<Variant>()
        .register::<Connected>()
        .register::<ProfileId>()
        .register::<ConnectionKind>()
        .register::<ConnectStep>()
        .register::<ConnectQuestion>()
        .register::<ConnectAnswer>()
        .register::<AttemptId>()
        .register::<SetUp>()
        .register::<MenuAction>()
        .register::<SavedConnections>()
        .register::<SavedRow>()
        .register::<LaunchRequest>();

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
    // Written as rendered, LF, because .gitattributes pins the file to LF.
    fs::write(bindings_path(), generated.as_bytes())
        .expect("ui/src/protocol.ts must be writable by the generator");
}
