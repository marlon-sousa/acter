//! Adapter: the operating system's own menu bar, rendered from the layout
//! [`system_menu`](acter_core::system_menu) decides, with a chosen item emitted as a
//! [`MenuAction`] on [`MENU_EVENT`].
//!
//! An empty layout never calls `Builder::menu`, so Windows, where a native menu freezes NVDA,
//! cannot acquire one here.

use acter_core::{MenuAction, MenuItem, Standard, SystemMenu, system_menu};
use tauri::menu::{IsMenuItem, Menu, MenuItemBuilder, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Builder, Emitter, Runtime};

pub(crate) const MENU_EVENT: &str = "acter://menu";

pub(crate) fn install<R: Runtime>(builder: Builder<R>, os: &str) -> Builder<R> {
    let layout = system_menu(os);
    if layout.is_empty() {
        return builder;
    }
    builder
        .menu(move |app| build(app, &layout))
        .on_menu_event(|app, event| {
            // An unrecognised id is a platform item the platform has already handled.
            if let Some(action) = action_of(event.id().as_ref()) {
                let _ = app.emit(MENU_EVENT, action);
            }
        })
}

fn id_of(action: MenuAction) -> &'static str {
    match action {
        MenuAction::Connect => "acter-connect",
        MenuAction::NewConnection => "acter-new-connection",
        MenuAction::SaveConnection => "acter-save-connection",
        MenuAction::Help => "acter-help",
        MenuAction::About => "acter-about",
    }
}

/// `None` means an item Acter did not put there.
fn action_of(id: &str) -> Option<MenuAction> {
    EVERY_ACTION.into_iter().find(|action| id_of(*action) == id)
}

const EVERY_ACTION: [MenuAction; 5] = [
    MenuAction::Connect,
    MenuAction::NewConnection,
    MenuAction::SaveConnection,
    MenuAction::Help,
    MenuAction::About,
];

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

/// `None` keeps the platform's own, already translated, label.
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

    #[test]
    fn every_action_survives_being_written_into_an_id_and_read_back() {
        for action in EVERY_ACTION {
            assert_eq!(action_of(id_of(action)), Some(action));
        }
    }

    #[test]
    fn no_two_actions_share_an_id() {
        let ids = EVERY_ACTION.map(id_of);
        let mut sorted = ids.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
    }

    #[test]
    fn an_item_acter_did_not_put_there_is_not_one_of_its_actions() {
        for id in ["quit", "copy", "", "acter-", "acter-connect-2"] {
            assert_eq!(action_of(id), None, "{id} was read as an Acter action");
        }
    }
}
