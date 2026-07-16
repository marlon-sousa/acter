//! Adapter: the echo Tauri router — a one-line `#[tauri::command]` delegating to
//! the harness through its port from managed state. Routers are the only Rust
//! modules shaped by Tauri's command signature conventions.

use tauri::{State, command};

use crate::container::AppState;

#[command]
pub(crate) fn echo(state: State<'_, AppState>, text: String) -> String {
    state.echo.echo(&text)
}
