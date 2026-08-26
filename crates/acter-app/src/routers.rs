//! Facade for this crate's routers, one file per router.
//!
//! Glob re-exports are required here: `#[tauri::command]` generates hidden
//! companion items (`__cmd__<name>` etc.) that `generate_handler!` resolves
//! alongside the function, and a named re-export would leave them behind.

mod about;
mod connect;
mod platform;
mod session;

pub(crate) use about::*;
pub(crate) use connect::*;
pub(crate) use platform::*;
pub(crate) use session::*;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use acter_core::{
        CommandId, ConnectApi, Connectable, Connected, KeyAck, ProfileId, SessionApi, SubmitAck,
    };
    use serde_json::{Value, json};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
    use tauri::webview::InvokeRequest;
    use tauri::{WebviewWindowBuilder, generate_handler};

    use crate::container::{AppState, state};

    /// The scripted far end these tests connect to when they want a session: a debug build
    /// offers it, and no process is spawned to run it.
    const BUILTIN: &str = "builtin";

    /// Builds the app on the Tauri mock runtime with the real connect service wired into
    /// managed state, then invokes `cmd` through the real IPC pipeline — the same path a
    /// webview `invoke` takes (registration, state extraction, argument deserialization),
    /// none of which unit tests reach. No sink is attached, so nothing a session produces
    /// has anywhere to go and only the invoke surface is exercised.
    ///
    /// `session` says whether to connect to the scripted far end first, which is the
    /// difference between the two windows B7 created: one with a session behind it and one
    /// with nothing. Both are built inside the async runtime for the reason the container
    /// does the same: a session starts tasks.
    fn invoke_with(session: bool, cmd: &str, args: Value) -> Result<Value, Value> {
        let runtime = tauri::async_runtime::handle();
        let state = {
            let _entered = runtime.inner().enter();
            let service = Arc::new(state());
            if session {
                service
                    .use_profile(&ProfileId::Scripted {
                        name: BUILTIN.to_owned(),
                    })
                    .expect("the built-in scripted session starts");
            }
            AppState {
                session: Arc::clone(&service) as Arc<dyn SessionApi>,
                connect: service as Arc<dyn ConnectApi>,
            }
        };
        let app = mock_builder()
            .manage(state)
            .invoke_handler(generate_handler![
                super::submit_command,
                super::attach_session,
                super::send_key,
                super::connectable,
                super::use_profile,
                super::connected
            ])
            .build(mock_context(noop_assets()))
            .expect("failed to build the mock app");
        let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build the mock webview");
        get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "http://tauri.localhost".parse().unwrap(),
                body: InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|body| body.deserialize::<Value>().expect("response was not JSON"))
    }

    /// A window with a session behind it.
    fn invoke(cmd: &str, args: Value) -> Result<Value, Value> {
        invoke_with(true, cmd, args)
    }

    /// A window connected to nothing, which is what an ordinary launch opens since B7.
    fn invoke_unconnected(cmd: &str, args: Value) -> Result<Value, Value> {
        invoke_with(false, cmd, args)
    }

    #[test]
    fn submit_command_returns_a_submit_ack_through_the_real_router() {
        let out = invoke("submit_command", json!({ "sessionId": 1, "line": "small" }))
            .expect("submit_command should succeed");
        let ack: SubmitAck = serde_json::from_value(out).expect("response should be a SubmitAck");
        assert_eq!(
            ack,
            SubmitAck::Accepted {
                command_id: CommandId(1)
            }
        );
    }

    /// The keystroke protocol over the wire: a `KeyPress` deserialized from the shape the
    /// frontend will send, and a `KeyAck` back. Nothing is running in a session nobody
    /// submitted to, which is one of the two answers the ack exists to give.
    #[test]
    fn send_key_takes_a_key_press_and_answers_a_key_ack() {
        let out = invoke(
            "send_key",
            json!({
                "sessionId": 1,
                "key": { "key": { "Char": "c" }, "ctrl": true, "shift": false, "alt": false }
            }),
        )
        .expect("send_key should succeed");
        let ack: KeyAck = serde_json::from_value(out).expect("response should be a KeyAck");
        assert_eq!(ack, KeyAck::NothingToActOn);
    }

    #[test]
    fn submit_command_missing_line_surfaces_an_error_not_a_panic() {
        let err = invoke("submit_command", json!({ "sessionId": 1 }))
            .expect_err("a missing `line` argument must surface as an error response");
        assert!(
            err.to_string().contains("line"),
            "error should name the missing argument, got: {err}"
        );
    }

    /// **The unconnected window, through the pipeline a real one uses.** A line typed into
    /// it comes back refused rather than acknowledged, so the frontend has something to say
    /// and something to decide — chiefly not to clear the field the user typed into.
    #[test]
    fn a_line_submitted_into_an_unconnected_window_is_refused_over_the_wire() {
        let out = invoke_unconnected("submit_command", json!({ "sessionId": 1, "line": "dir" }))
            .expect("a refusal is an answer, not an error");
        let ack: SubmitAck = serde_json::from_value(out).expect("response should be a SubmitAck");

        assert_eq!(ack, SubmitAck::NotConnected);
    }

    /// And the same window asked what it is connected to: nothing, which is what makes the
    /// frontend announce that it is empty and say where to go.
    #[test]
    fn an_unconnected_window_answers_no_connection() {
        let out = invoke_unconnected("connected", json!({})).expect("connected should succeed");

        assert_eq!(out, Value::Null);
    }

    /// A session that was started answers with the id every later invoke carries and the
    /// label the window names itself with.
    #[test]
    fn a_connected_window_answers_which_far_end_it_is_on() {
        let out = invoke("connected", json!({})).expect("connected should succeed");
        let connected: Connected =
            serde_json::from_value(out).expect("response should be a Connected");

        assert_eq!(connected.label, "Scripted: builtin");
    }

    /// The list crosses the wire whole. What is in it depends on the machine running the
    /// suite, so what is asserted is the shape every row has and the one entry a debug
    /// build always offers.
    #[test]
    fn connectable_lists_rows_the_frontend_can_render() {
        let out = invoke_unconnected("connectable", json!({})).expect("connectable should succeed");
        let listed: Vec<Connectable> =
            serde_json::from_value(out).expect("response should be a list of Connectable");

        assert!(
            listed.iter().all(|row| !row.label.trim().is_empty()),
            "every row is named, because the list is navigated by ear"
        );
        assert!(
            listed
                .iter()
                .all(|row| row.available == row.instructions.is_none()),
            "a row that cannot be used explains itself, and one that can does not"
        );
        assert!(
            listed.iter().any(|row| row.id
                == ProfileId::Scripted {
                    name: BUILTIN.to_owned()
                }),
            "a debug build offers the scripted sessions: {listed:?}"
        );
    }

    /// **Connecting, end to end through the IPC pipeline**: the profile is deserialized from
    /// the shape the frontend sends, a session is started, and the id it answers with is the
    /// one a submitted line then has to carry.
    #[test]
    fn use_profile_starts_a_session_and_answers_the_id_that_reaches_it() {
        let out = invoke_unconnected(
            "use_profile",
            json!({ "profile": { "profile": "Scripted", "name": BUILTIN } }),
        )
        .expect("the built-in scripted session starts");
        let connected: Connected =
            serde_json::from_value(out).expect("response should be a Connected");

        assert_eq!(connected.label, "Scripted: builtin");
        assert_eq!(connected.session.0, 1, "the first session of this window");
    }

    /// A profile that cannot be started comes back as a *rejected promise carrying a
    /// sentence*, which is what the frontend says out loud. Not a panic, and not an empty
    /// error a listener would meet as silence.
    #[test]
    fn a_profile_that_cannot_be_started_rejects_with_a_speakable_sentence() {
        let err = invoke(
            "use_profile",
            json!({ "profile": { "profile": "Scripted", "name": "no-such-transcript.json" } }),
        )
        .expect_err("there is no such transcript");
        let said = err.as_str().expect("the rejection is a sentence");

        assert!(said.ends_with('.'), "a spoken message ends: {said}");
        assert!(
            said.split_whitespace().count() >= 5,
            "a spoken message says what happened, not a label: {said}"
        );
    }
}
