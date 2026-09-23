//! Facade for this crate's routers, one file per router.
//!
//! Glob re-exports are required: `#[tauri::command]` generates hidden companion items that
//! `generate_handler!` resolves alongside the function, and a named re-export leaves them behind.

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

    use crate::adapters::Settings;
    use crate::container::{AppState, SettingsFolder, Standing, Version, state};

    const BUILTIN: &str = "builtin";

    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    fn invoke_with(session: bool, cmd: &str, args: Value) -> Result<Value, Value> {
        let mock = mock_app(session);
        invoke_on(&mock, cmd, args)
    }

    struct Mock {
        webview: tauri::WebviewWindow<tauri::test::MockRuntime>,
        service: Arc<ConnectService>,
        _app: tauri::App<tauri::test::MockRuntime>,
    }

    fn mock_app(session: bool) -> Mock {
        let runtime = tauri::async_runtime::handle();
        let settings = Arc::new(Settings::open(
            SettingsFolder {
                path: std::env::temp_dir().join(format!(
                    "acter-routers-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                )),
                standing: Standing::Directed,
            },
            Version::development("in-a-test"),
        ));
        // Entered because starting a session spawns tasks.
        let service = {
            let _entered = runtime.inner().enter();
            let service = Arc::new(state(&settings));
            if session {
                service
                    .use_profile(
                        &ProfileId::Scripted {
                            name: BUILTIN.to_owned(),
                        },
                        acter_core::SetUp::Yes,
                        None,
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
            settings,
        };
        let app = mock_builder()
            .manage(state)
            .invoke_handler(generate_handler![
                super::submit_command,
                super::attach_session,
                super::send_key,
                super::set_line_owner,
                super::paste,
                super::connectable,
                super::use_profile,
                super::answer_connect,
                super::attempt_ended,
                super::connected,
                super::about,
                super::saved,
                super::save_connection,
                super::rename_connection,
                super::forget_connection,
                super::offer_to_save,
                super::stop_offering_to_save,
                super::requested_at_launch
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

    fn invoke(cmd: &str, args: Value) -> Result<Value, Value> {
        invoke_with(true, cmd, args)
    }

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

    #[test]
    fn a_line_submitted_into_an_unconnected_window_is_refused_over_the_wire() {
        let out = invoke_unconnected("submit_command", json!({ "sessionId": 1, "line": "dir" }))
            .expect("a refusal is an answer, not an error");
        let ack: SubmitAck = serde_json::from_value(out).expect("response should be a SubmitAck");

        assert_eq!(ack, SubmitAck::NotConnected);
    }

    #[test]
    fn an_unconnected_window_answers_no_connection() {
        let out = invoke_unconnected("connected", json!({})).expect("connected should succeed");

        assert_eq!(out, Value::Null);
    }

    #[test]
    fn a_connected_window_answers_which_far_end_it_is_on() {
        let out = invoke("connected", json!({})).expect("connected should succeed");
        let connected: Connected =
            serde_json::from_value(out).expect("response should be a Connected");

        assert_eq!(connected.label, "Scripted: builtin");
    }

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

    #[test]
    fn use_profile_answers_an_attempt_id_and_starts_the_work_behind_it() {
        let mock = mock_app(false);

        let out = invoke_on(
            &mock,
            "use_profile",
            json!({
                "profile": { "profile": "Scripted", "name": BUILTIN },
                "setUp": "Yes",
                "origin": null,
                "steps": "__CHANNEL__:1",
            }),
        )
        .expect("the attempt starts");

        let attempt: AttemptId = serde_json::from_value(out).expect("an attempt id comes back");
        assert_eq!(attempt.0, 1, "the first attempt of this window");

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

    #[test]
    fn about_answers_the_build_the_version_and_the_settings_folder() {
        let out = invoke_unconnected("about", json!({})).expect("about should succeed");

        assert_eq!(out["name"], "Acter");
        let folder = out["settings_folder"]
            .as_str()
            .expect("the settings folder is a path a user can read out");
        assert!(
            !folder.trim().is_empty(),
            "a folder nobody can name is no answer"
        );
        for line in ["settings_standing", "version_said"] {
            let said = out[line].as_str().expect("{line} is a sentence");
            assert!(said.ends_with('.'), "it is read aloud: {said}");
            assert!(!said.contains("  "), "with no run of spaces: {said}");
        }
        assert!(
            !out["version"]
                .as_str()
                .expect("a version")
                .trim()
                .is_empty(),
            "a bug report has something to carry"
        );
    }

    #[test]
    fn saved_answers_rows_the_dialog_can_render() {
        let out = invoke_unconnected("saved", json!({})).expect("saved should succeed");

        assert!(
            out["rows"].as_array().expect("rows is a list").is_empty(),
            "nothing is saved in a fixture nobody wrote to"
        );
        assert_eq!(
            out["unreadable"],
            Value::Null,
            "an ordinary first run has nothing to report"
        );
    }

    #[test]
    fn saving_with_nothing_connected_is_refused_over_the_wire_in_a_sentence() {
        let why = invoke_unconnected("save_connection", json!({ "name": "work laptop" }))
            .expect_err("there is nothing to save");

        assert_eq!(
            why.as_str().expect("a sentence"),
            "Nothing is connected, so there is nothing to save."
        );
    }

    #[test]
    fn a_connection_is_saved_renamed_and_forgotten_through_the_real_invokes() {
        let mock = mock_app(true);

        let saved =
            invoke_on(&mock, "save_connection", json!({ "name": "the fake" })).expect("it saves");
        assert_eq!(saved, "Saved as the fake.");

        let listed = invoke_on(&mock, "saved", json!({})).expect("saved should succeed");
        let rows = listed["rows"].as_array().expect("rows is a list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "the fake");
        assert_eq!(rows[0]["summary"], "Scripted, builtin");
        assert_eq!(rows[0]["available"], true);

        let renamed = invoke_on(
            &mock,
            "rename_connection",
            json!({ "from": "the fake", "to": "the other fake" }),
        )
        .expect("it renames");
        assert_eq!(renamed, "the fake is now called the other fake.");

        let forgotten = invoke_on(
            &mock,
            "forget_connection",
            json!({ "name": "the other fake" }),
        )
        .expect("it forgets");
        assert_eq!(forgotten, "the other fake is no longer saved.");

        let listed = invoke_on(&mock, "saved", json!({})).expect("saved should succeed");
        assert!(
            listed["rows"]
                .as_array()
                .expect("rows is a list")
                .is_empty()
        );
    }

    #[test]
    fn the_offer_to_save_is_asked_and_answered_over_the_wire() {
        let mock = mock_app(false);

        let before = invoke_on(&mock, "offer_to_save", json!({})).expect("it is asked");
        assert_eq!(before, Value::Bool(true), "nobody has said otherwise");

        invoke_on(&mock, "stop_offering_to_save", json!({})).expect("the box is ticked");

        let after = invoke_on(&mock, "offer_to_save", json!({})).expect("it is asked again");
        assert_eq!(after, Value::Bool(false));
    }

    /// Holds only while the test binary's own command line carries no `--connect`.
    #[test]
    fn an_ordinary_launch_carries_no_request_over_the_wire() {
        let out = invoke_unconnected("requested_at_launch", json!({}))
            .expect("requested_at_launch should succeed");

        assert_eq!(out, Value::Null);
    }
}
