//! Facade for this crate's routers, one file per router.
//!
//! Glob re-exports are required here: `#[tauri::command]` generates hidden
//! companion items (`__cmd__<name>` etc.) that `generate_handler!` resolves
//! alongside the function, and a named re-export would leave them behind.

mod echo;

pub(crate) use echo::*;
