//! Controller (orchestrator): the container (composition root) — the only place
//! where concrete implementations are constructed and bound to their ports, and
//! where the Tauri runtime is started.

use std::sync::Arc;

use tauri::{Builder, generate_context, generate_handler};

use crate::ports::EchoApi;
use crate::services::EchoService;

pub(crate) struct AppState {
    pub(crate) echo: Arc<dyn EchoApi>,
}

pub fn run() {
    let state = AppState {
        echo: Arc::new(EchoService),
    };
    let builder = Builder::default()
        .manage(state)
        .invoke_handler(generate_handler![crate::routers::echo]);

    // Embedded WebDriver server for E2E tests (spec T2): debug builds only, so
    // release binaries carry no automation surface. Debug builds exist only on
    // developer machines and CI.
    #[cfg(debug_assertions)]
    let builder = {
        use tauri_plugin_wdio_webdriver::init;
        builder.plugin(init())
    };

    builder
        .run(generate_context!())
        .expect("failed to start the Acter window");
}
