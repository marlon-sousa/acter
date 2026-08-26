//! SPIKE (A7's opening measurement, not shipped): a window carrying the menu bar and an
//! empty page, so that the stall measured against the real window can be attributed.
//!
//! Two windows are possible from here, chosen by ACTER_SPIKE_PAGE:
//!   unset / "blank" — one window on about:blank, menu bar attached, nothing else.
//!   "app"           — the configured Acter page, menu bar attached, no session behind it.
//!
//! Reverted once the measurement is written into the spec.

use tauri::menu::{MenuBuilder, SubmenuBuilder};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, generate_context};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let blank =
                std::env::var("ACTER_SPIKE_PAGE").unwrap_or_else(|_| "blank".to_owned()) != "app";

            let window = if blank {
                // The configured window carries the real page; close it and open an empty one.
                if let Some(configured) = app.get_webview_window("main") {
                    configured.close()?;
                }
                WebviewWindowBuilder::new(
                    app,
                    "spike",
                    WebviewUrl::External("about:blank".parse().expect("about:blank parses")),
                )
                .title("Acter menu spike")
                .inner_size(700.0, 400.0)
                .build()?
            } else {
                app.get_webview_window("main")
                    .expect("the main window exists")
            };

            let acter = SubmenuBuilder::new(app, "Acter")
                .text("acter-exit", "Exit")
                .build()?;
            let about = SubmenuBuilder::new(app, "About")
                .text("about-acter", "About Acter")
                .build()?;
            let menu = MenuBuilder::new(app).items(&[&acter, &about]).build()?;
            window.set_menu(menu)?;
            window.set_focus()?;
            window.on_menu_event(|_window, event| {
                println!("SPIKE menu event: {:?}", event.id());
            });
            Ok(())
        })
        .run(generate_context!())
        .expect("failed to start the spike window");
}
