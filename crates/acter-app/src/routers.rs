//! Facade for this crate's routers, one file per router.
//!
//! Glob re-exports are required here: `#[tauri::command]` generates hidden
//! companion items (`__cmd__<name>` etc.) that `generate_handler!` resolves
//! alongside the function, and a named re-export would leave them behind.

mod echo;

pub(crate) use echo::*;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
    use tauri::webview::InvokeRequest;
    use tauri::{WebviewWindowBuilder, generate_handler};

    use crate::container::AppState;
    use crate::services::EchoService;

    /// Builds the app on the Tauri mock runtime with the real service wired into
    /// managed state, then invokes `cmd` through the real IPC pipeline — the same
    /// path a webview `invoke` takes: command registration, state extraction, and
    /// argument deserialization, none of which unit tests reach.
    fn invoke(cmd: &str, args: Value) -> Result<String, Value> {
        let state = AppState {
            echo: Arc::new(EchoService),
        };
        let app = mock_builder()
            .manage(state)
            .invoke_handler(generate_handler![super::echo])
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
        .map(|body| {
            body.deserialize::<String>()
                .expect("echo response was not a string")
        })
    }

    #[test]
    fn echo_round_trips_through_the_real_router() {
        let out = invoke("echo", json!({ "text": "git status" })).expect("echo should succeed");
        assert_eq!(out, "git status");
    }

    #[test]
    fn missing_argument_surfaces_an_error_not_a_panic() {
        let err = invoke("echo", json!({}))
            .expect_err("a missing `text` argument must surface as an error response");
        assert!(
            err.to_string().contains("text"),
            "error should name the missing argument, got: {err}"
        );
    }
}
