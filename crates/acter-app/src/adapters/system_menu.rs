//! Adapter: the operating system's own menu bar — the layout
//! [`system_menu`](acter_core::system_menu) decided, rendered into Tauri's menu, and a
//! chosen item turned into an event the window can act on.
//!
//! **It builds a menu only for a platform that asked for one.** The layout is empty on
//! Windows and on Linux, and an empty layout means `Builder::menu` is never called — so the
//! platform where a native menu freezes NVDA for tens of seconds (spec A7) cannot acquire
//! one by an edit here. Calling it at all is also what turns Tauri's own default menu off:
//! `Builder::build` installs `Menu::default` on macOS only when nobody set a menu, and that
//! default is the one measured on 2026-09-02 with an empty Help submenu and no Connect
//! anywhere (spec M3).
//!
//! **Not `#[cfg]`-gated, because nothing here is per-platform.** Tauri's menu API is the
//! same on every desktop, and which menu to build is a value the domain answers — so this
//! module compiles and is tested everywhere, and the divergence stays where ARCHITECTURE
//! wants it: in a policy taking the operating system's name.
//!
//! # What a chosen item does
//!
//! The items the platform owns — quit, hide, copy, minimise — are answered by the platform
//! and never reach this code. The three Acter answers itself open dialogs the *frontend*
//! owns, so choosing one emits a single event carrying a [`MenuAction`]; `routers/tauri.ts`
//! is the one place that listens, and the frontend's switch over the action is exhaustive.

use acter_core::{MenuAction, MenuItem, Standard, SystemMenu, system_menu};
use tauri::menu::{IsMenuItem, Menu, MenuItemBuilder, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Builder, Emitter, Runtime};

/// The event a chosen item arrives on. One event for every action rather than one event
/// each: the payload says which, and a frontend that must handle them all in one place is a
/// frontend where an unhandled one is a compile error.
pub(crate) const MENU_EVENT: &str = "acter://menu";

/// Give the builder the menu this operating system asked for, or hand it back untouched.
pub(crate) fn install<R: Runtime>(builder: Builder<R>, os: &str) -> Builder<R> {
    let layout = system_menu(os);
    if layout.is_empty() {
        return builder;
    }
    builder
        .menu(move |app| build(app, &layout))
        .on_menu_event(|app, event| {
            // An id this does not recognise is a platform item, which the platform has
            // already dealt with. Nothing to report and nothing to log.
            if let Some(action) = action_of(event.id().as_ref()) {
                // A window that has gone is the ordinary case at quit time, not a fault.
                let _ = app.emit(MENU_EVENT, action);
            }
        })
}

/// The id an item carries, and the one thing in this file with two directions to keep in
/// step — which is why both are tested rather than only the one that is called at startup.
fn id_of(action: MenuAction) -> &'static str {
    match action {
        MenuAction::Connect => "acter-connect",
        MenuAction::Help => "acter-help",
        MenuAction::About => "acter-about",
    }
}

/// Which action an id names, or `None` for an item Acter did not put there.
fn action_of(id: &str) -> Option<MenuAction> {
    [MenuAction::Connect, MenuAction::Help, MenuAction::About]
        .into_iter()
        .find(|action| id_of(*action) == id)
}

fn build<R: Runtime>(app: &AppHandle<R>, layout: &[SystemMenu]) -> tauri::Result<Menu<R>> {
    let menus = layout
        .iter()
        .map(|menu| submenu(app, menu))
        .collect::<tauri::Result<Vec<_>>>()?;
    let refs: Vec<&dyn IsMenuItem<R>> = menus
        .iter()
        .map(|menu| menu as &dyn IsMenuItem<R>)
        .collect();
    Menu::with_items(app, &refs)
}

fn submenu<R: Runtime>(app: &AppHandle<R>, menu: &SystemMenu) -> tauri::Result<Submenu<R>> {
    let items = menu
        .items
        .iter()
        .map(|item| line(app, item))
        .collect::<tauri::Result<Vec<_>>>()?;
    let refs: Vec<&dyn IsMenuItem<R>> = items.iter().map(AsRef::as_ref).collect();
    Submenu::with_items(app, menu.title, true, &refs)
}

fn line<R: Runtime>(app: &AppHandle<R>, item: &MenuItem) -> tauri::Result<Box<dyn IsMenuItem<R>>> {
    Ok(match item {
        MenuItem::Separator => Box::new(PredefinedMenuItem::separator(app)?),
        MenuItem::Standard(standard) => platform_item(app, *standard)?,
        MenuItem::Acter {
            action,
            label,
            accelerator,
        } => {
            let mut builder = MenuItemBuilder::with_id(id_of(*action), label);
            if let Some(keys) = accelerator {
                builder = builder.accelerator(keys);
            }
            Box::new(builder.build(app)?)
        }
    })
}

/// **`None` everywhere, and that is decision 4 of the spec**: the platform's items keep the
/// platform's words, which macOS has already translated into the language the account is
/// set to. Passing English through would replace a translation with a string nobody asked
/// for, on a machine whose owner runs it in Portuguese.
fn platform_item<R: Runtime>(
    app: &AppHandle<R>,
    standard: Standard,
) -> tauri::Result<Box<dyn IsMenuItem<R>>> {
    Ok(match standard {
        Standard::Services => Box::new(PredefinedMenuItem::services(app, None)?),
        Standard::Hide => Box::new(PredefinedMenuItem::hide(app, None)?),
        Standard::HideOthers => Box::new(PredefinedMenuItem::hide_others(app, None)?),
        Standard::ShowAll => Box::new(PredefinedMenuItem::show_all(app, None)?),
        Standard::Quit => Box::new(PredefinedMenuItem::quit(app, None)?),
        Standard::Undo => Box::new(PredefinedMenuItem::undo(app, None)?),
        Standard::Redo => Box::new(PredefinedMenuItem::redo(app, None)?),
        Standard::Cut => Box::new(PredefinedMenuItem::cut(app, None)?),
        Standard::Copy => Box::new(PredefinedMenuItem::copy(app, None)?),
        Standard::Paste => Box::new(PredefinedMenuItem::paste(app, None)?),
        Standard::SelectAll => Box::new(PredefinedMenuItem::select_all(app, None)?),
        Standard::CloseWindow => Box::new(PredefinedMenuItem::close_window(app, None)?),
        Standard::Minimize => Box::new(PredefinedMenuItem::minimize(app, None)?),
        Standard::Maximize => Box::new(PredefinedMenuItem::maximize(app, None)?),
        Standard::Fullscreen => Box::new(PredefinedMenuItem::fullscreen(app, None)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The round trip, in the direction the running application uses it: an item is built
    /// with an id and comes back as the action it was built from.
    #[test]
    fn every_action_survives_being_written_into_an_id_and_read_back() {
        for action in [MenuAction::Connect, MenuAction::Help, MenuAction::About] {
            assert_eq!(action_of(id_of(action)), Some(action));
        }
    }

    /// Two actions sharing an id would make one of them unreachable, and the menu would
    /// look right while doing the wrong thing.
    #[test]
    fn no_two_actions_share_an_id() {
        let ids = [MenuAction::Connect, MenuAction::Help, MenuAction::About].map(id_of);
        let mut sorted = ids.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
    }

    /// What the platform's own items produce. `quit` and `copy` are answered by macOS and
    /// must not be reported to the window as though Acter had been asked something.
    #[test]
    fn an_item_acter_did_not_put_there_is_not_one_of_its_actions() {
        for id in ["quit", "copy", "", "acter-", "acter-connect-2"] {
            assert_eq!(action_of(id), None, "{id} was read as an Acter action");
        }
    }
}
