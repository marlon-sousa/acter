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

    use std::time::{Duration, Instant};

    use acter_core::{
        AttemptId, CommandId, ConnectApi, ConnectService, Connectable, Connected, KeyAck,
        ProfileId, SessionApi, SubmitAck,
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
        let mock = mock_app(session);
        invoke_on(&mock, cmd, args)
    }

    /// A built app, its webview, and the connect service behind its state.
    ///
    /// **Held together, and the service held twice**, so a test can invoke more than once
    /// against one window and then ask the service what became of it — which is what
    /// connecting needs since B9, because the answer arrives behind the invoke rather than
    /// in it.
    struct Mock {
        webview: tauri::WebviewWindow<tauri::test::MockRuntime>,
        service: Arc<ConnectService>,
        _app: tauri::App<tauri::test::MockRuntime>,
    }

    fn mock_app(session: bool) -> Mock {
        let runtime = tauri::async_runtime::handle();
        let service = {
            let _entered = runtime.inner().enter();
            let service = Arc::new(state());
            if session {
                service
                    .use_profile(
                        &ProfileId::Scripted {
                            name: BUILTIN.to_owned(),
                        },
                        acter_core::SetUp::Yes,
                        &(Arc::new(acter_core::Unasked) as Arc<dyn acter_core::ConnectQuestions>),
                    )
                    .expect("the built-in scripted session starts");
            }
            service
        };
        let connect = Arc::clone(&service) as Arc<dyn ConnectApi>;
        let state = AppState {
            session: Arc::clone(&service) as Arc<dyn SessionApi>,
            connecting: Arc::new(crate::controllers::Connecting::new(Arc::clone(&connect))),
            connect,
        };
        let app = mock_builder()
            .manage(state)
            .invoke_handler(generate_handler![
                super::submit_command,
                super::attach_session,
                super::send_key,
                super::connectable,
                super::use_profile,
                super::answer_connect,
                super::attempt_ended,
                super::connected
            ])
            .build(mock_context(noop_assets()))
            .expect("failed to build the mock app");
        let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build the mock webview");
        Mock {
            webview,
            service,
            _app: app,
        }
    }

    fn invoke_on(mock: &Mock, cmd: &str, args: Value) -> Result<Value, Value> {
        get_ipc_response(
            &mock.webview,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: mock.webview.url().expect("the mock webview has a url"),
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

    /// **Connecting, end to end through the IPC pipeline**: the profile and the channel are
    /// deserialized from the shape the frontend sends, and an attempt id comes back at once.
    ///
    /// **It answers with an id rather than a session, and that is the change B9 made.**
    /// Connecting can stop partway to ask a person something, so the invoke cannot be the
    /// thing that answers — the session arrives later, as a step. What this asserts is that
    /// the invoke returns immediately and that the work really did start behind it.
    #[test]
    fn use_profile_answers_an_attempt_id_and_starts_the_work_behind_it() {
        let mock = mock_app(false);

        let out = invoke_on(
            &mock,
            "use_profile",
            json!({
                "profile": { "profile": "Scripted", "name": BUILTIN },
                // The Connect dialog's checkbox, which travels with the attempt (spec B9.5,
                // decision 9). Ticked is what the dialog sends by default.
                "setUp": "Yes",
                "steps": "__CHANNEL__:1",
            }),
        )
        .expect("the attempt starts");

        let attempt: AttemptId = serde_json::from_value(out).expect("an attempt id comes back");
        assert_eq!(attempt.0, 1, "the first attempt of this window");

        // The session appears behind the invoke rather than in it, so this waits for it.
        let deadline = Instant::now() + Duration::from_secs(10);
        let connected = loop {
            if let Some(connected) = mock.service.connected() {
                break connected;
            }
            assert!(
                Instant::now() < deadline,
                "the scripted session never started"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(connected.label, "Scripted: builtin");
        assert_eq!(connected.session.0, 1, "the first session of this window");
    }

    /// An answer for an attempt nobody is running is ignored rather than rejected: it is the
    /// ordinary consequence of a dialog abandoned or a window reloaded, and the invoke still
    /// has to return normally.
    #[test]
    fn an_answer_for_an_attempt_that_is_not_running_is_ignored() {
        let mock = mock_app(false);

        invoke_on(
            &mock,
            "answer_connect",
            json!({ "attempt": 99, "answer": { "answer": "Trust" } }),
        )
        .expect("answering something stale is not an error");
        invoke_on(&mock, "attempt_ended", json!({ "attempt": 99 }))
            .expect("ending something stale is not an error");
    }
}
