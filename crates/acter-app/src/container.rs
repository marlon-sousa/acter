//! Controller (orchestrator): the container (composition root) — the only place
//! where concrete implementations are constructed and bound to their ports, where the
//! environment is read, and where the Tauri runtime is started.

use std::env;
use std::fs;
use std::sync::Arc;

use acter_core::SessionApi;
use tauri::{Builder, generate_context, generate_handler};

use crate::entities::{self, FakeScript};
use crate::services::FakeSessionService;

/// The environment variable pointing at an optional fake script config JSON file. When
/// unset, the built-in defaults (human-scale manual-testing numbers) are used.
const FAKE_SCRIPT_ENV: &str = "ACTER_FAKE_SCRIPT";

pub(crate) struct AppState {
    pub(crate) session: Arc<dyn SessionApi>,
}

pub fn run() {
    let script = load_fake_script();
    let state = AppState {
        session: Arc::new(FakeSessionService::new(script)),
    };
    let builder = Builder::default()
        .manage(state)
        .invoke_handler(generate_handler![
            crate::routers::attach_session,
            crate::routers::submit_command
        ]);

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

/// Loads the fake script config: the JSON file named by `ACTER_FAKE_SCRIPT` if set,
/// otherwise the built-in defaults. Reading the environment and the filesystem is the
/// container's privilege — this is the composition root, where the world is allowed in.
/// A parse or read failure is a loud startup error, never a silent fallback.
fn load_fake_script() -> FakeScript {
    let Ok(path) = env::var(FAKE_SCRIPT_ENV) else {
        return FakeScript::default();
    };
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Acter could not read the fake script config at {path}: {e}"));
    entities::parse(&contents)
        .unwrap_or_else(|e| panic!("Acter could not load the fake script config at {path}: {e}"))
}
