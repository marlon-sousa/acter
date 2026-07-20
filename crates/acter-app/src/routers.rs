//! Facade for this crate's routers, one file per router.
//!
//! Glob re-exports are required here: `#[tauri::command]` generates hidden
//! companion items (`__cmd__<name>` etc.) that `generate_handler!` resolves
//! alongside the function, and a named re-export would leave them behind.

mod session;

pub(crate) use session::*;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use acter_core::{CommandId, SubmitAck};
    use serde_json::{Value, json};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
    use tauri::webview::InvokeRequest;
    use tauri::{WebviewWindowBuilder, generate_handler};

    use crate::container::AppState;
    use crate::entities::FakeScript;
    use crate::services::FakeSessionService;

    /// Builds the app on the Tauri mock runtime with the real `FakeSessionService`
    /// wired into managed state, then invokes `cmd` through the real IPC pipeline —
    /// the same path a webview `invoke` takes (registration, state extraction,
    /// argument deserialization), none of which unit tests reach. No sink is attached,
    /// so `submit_command` spawns no playback thread and only the ack is exercised.
    fn invoke(cmd: &str, args: Value) -> Result<Value, Value> {
        let state = AppState {
            session: Arc::new(FakeSessionService::new(FakeScript::default())),
        };
        let app = mock_builder()
            .manage(state)
            .invoke_handler(generate_handler![
                super::submit_command,
                super::attach_session
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

    #[test]
    fn submit_command_returns_a_submit_ack_through_the_real_router() {
        let out = invoke("submit_command", json!({ "sessionId": 1, "line": "small" }))
            .expect("submit_command should succeed");
        let ack: SubmitAck = serde_json::from_value(out).expect("response should be a SubmitAck");
        assert_eq!(ack.command_id, CommandId(1));
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
}
