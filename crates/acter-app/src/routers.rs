//! Adapter: Tauri routers — one-line `#[tauri::command]` functions delegating to
//! controllers/services through traits held in managed state. The only Rust module
//! shaped by Tauri's command signature conventions.

use tauri::State;

use crate::composition::AppState;

#[tauri::command]
pub(crate) fn echo(state: State<'_, AppState>, text: String) -> String {
    state.echo.echo(&text)
}
