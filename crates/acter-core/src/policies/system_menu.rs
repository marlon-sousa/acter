//! Policy: what the operating system's own menu bar holds, if that operating system has
//! one Acter puts anything in.
//!
//! A native menu freezes NVDA 2026.1.1 on Windows for tens of seconds, so an empty answer here
//! is what keeps the composition root from attaching one.
//!
//! Acter's own items carry Acter's words; the platform's items keep the platform's own
//! localized words, since macOS translates Cut, Paste, Minimise and Quit into the
//! account's language.
//!
//! On macOS an unbundled build's process name is `acter-app`, so platform items read
//! "Quit acter-app" rather than "Quit Acter".

use crate::MenuAction;

/// One menu in the bar, in the order the bar lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemMenu {
    /// The first menu is the application menu; macOS renders its title in bold as the
    /// application's own name.
    pub title: &'static str,
    pub items: Vec<MenuItem>,
}

/// One line in a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    /// An item Acter answers itself: choosing it reaches the frontend as a [`MenuAction`].
    Acter {
        action: MenuAction,
        label: &'static str,
        /// `None` when there is no shortcut. Never a lone function key: a Mac with
        /// factory settings needs `fn` to reach those.
        accelerator: Option<&'static str>,
    },
    /// An item the platform owns, including its words and its conventional shortcut.
    Standard(Standard),
    /// Not focusable; a screen reader passes over it.
    Separator,
}

/// The items every application on the platform has, done by the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Standard {
    Services,
    Hide,
    HideOthers,
    ShowAll,
    /// On macOS, taking this quits the running shell along with the app.
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    /// Ends the application for Acter.
    CloseWindow,
    Minimize,
    /// macOS's Zoom.
    Maximize,
    Fullscreen,
}

/// What this operating system's menu bar holds, or nothing if its menu is not there.
///
/// Windows returns nothing because its menu lives in the document; Linux because nobody
/// has decided yet.
pub fn system_menu(os: &str) -> Vec<SystemMenu> {
    match os {
        "macos" => macos(),
        _ => Vec::new(),
    }
}

fn macos() -> Vec<SystemMenu> {
    vec![
        SystemMenu {
            title: "Acter",
            items: vec![
                // Reads name, version, copyright and licence; the native panel only has
                // the first two.
                MenuItem::Acter {
                    action: MenuAction::About,
                    label: "About Acter",
                    accelerator: None,
                },
                MenuItem::Separator,
                MenuItem::Standard(Standard::Services),
                MenuItem::Separator,
                MenuItem::Standard(Standard::Hide),
                MenuItem::Standard(Standard::HideOthers),
                MenuItem::Standard(Standard::ShowAll),
                MenuItem::Separator,
                MenuItem::Standard(Standard::Quit),
            ],
        },
        SystemMenu {
            title: "File",
            items: vec![
                // Cmd+K is macOS's own "connect to server" shortcut.
                MenuItem::Acter {
                    action: MenuAction::Connect,
                    label: "Connect…",
                    accelerator: Some("CmdOrCtrl+K"),
                },
                // CmdOrCtrl+N and CmdOrCtrl+S are macOS's own new/save shortcuts in every
                // application.
                MenuItem::Acter {
                    action: MenuAction::NewConnection,
                    label: "New connection…",
                    accelerator: Some("CmdOrCtrl+N"),
                },
                MenuItem::Acter {
                    action: MenuAction::SaveConnection,
                    label: "Save connection…",
                    accelerator: Some("CmdOrCtrl+S"),
                },
                MenuItem::Separator,
                MenuItem::Standard(Standard::CloseWindow),
            ],
        },
        // macOS routes the webview's own copy and paste through the menu bar; without these
        // items Cmd+C stops working in the terminal.
        SystemMenu {
            title: "Edit",
            items: vec![
                MenuItem::Standard(Standard::Undo),
                MenuItem::Standard(Standard::Redo),
                MenuItem::Separator,
                MenuItem::Standard(Standard::Cut),
                MenuItem::Standard(Standard::Copy),
                MenuItem::Standard(Standard::Paste),
                MenuItem::Standard(Standard::SelectAll),
            ],
        },
        SystemMenu {
            title: "View",
            items: vec![MenuItem::Standard(Standard::Fullscreen)],
        },
        SystemMenu {
            title: "Window",
            items: vec![
                MenuItem::Standard(Standard::Minimize),
                MenuItem::Standard(Standard::Maximize),
                MenuItem::Separator,
                MenuItem::Standard(Standard::CloseWindow),
            ],
        },
        // Cmd+Slash, not the usual Cmd+? for Help: macOS reserves Shift+Cmd+/ for the
        // menu's own search field and intercepts it before the app sees it.
        SystemMenu {
            title: "Help",
            items: vec![MenuItem::Acter {
                action: MenuAction::Help,
                label: "Acter Help",
                accelerator: Some("CmdOrCtrl+Slash"),
            }],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actions(menus: &[SystemMenu]) -> Vec<MenuAction> {
        menus
            .iter()
            .flat_map(|menu| &menu.items)
            .filter_map(|item| match item {
                MenuItem::Acter { action, .. } => Some(*action),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn windows_asks_for_no_native_menu_because_its_menu_is_in_the_document() {
        assert!(system_menu("windows").is_empty());
    }

    #[test]
    fn a_platform_with_no_answer_yet_says_nothing_rather_than_guessing() {
        assert!(system_menu("linux").is_empty());
        assert!(system_menu("freebsd").is_empty());
    }

    #[test]
    fn macos_has_the_six_menus_the_platform_expects() {
        let titles: Vec<_> = system_menu("macos").iter().map(|menu| menu.title).collect();
        assert_eq!(titles, ["Acter", "File", "Edit", "View", "Window", "Help"]);
    }

    #[test]
    fn no_menu_is_empty_and_help_least_of_all() {
        for menu in system_menu("macos") {
            assert!(
                !menu.items.is_empty(),
                "the {} menu opens onto nothing",
                menu.title
            );
        }
    }

    #[test]
    fn connect_new_connection_and_save_connection_are_the_file_menu() {
        let acters: Vec<MenuAction> = file_menu()
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Acter { action, .. } => Some(*action),
                _ => None,
            })
            .collect();

        assert_eq!(
            acters,
            [
                MenuAction::Connect,
                MenuAction::NewConnection,
                MenuAction::SaveConnection
            ]
        );
    }

    #[test]
    fn new_and_save_take_the_shortcuts_a_mac_user_already_presses() {
        let file = file_menu();

        for (action, keys) in [
            (MenuAction::NewConnection, "CmdOrCtrl+N"),
            (MenuAction::SaveConnection, "CmdOrCtrl+S"),
        ] {
            let found = file.items.iter().find_map(|item| match item {
                MenuItem::Acter {
                    action: named,
                    accelerator,
                    ..
                } if *named == action => *accelerator,
                _ => None,
            });
            assert_eq!(found, Some(keys), "{action:?}");
        }
    }

    fn file_menu() -> SystemMenu {
        system_menu("macos")
            .into_iter()
            .find(|menu| menu.title == "File")
            .expect("macOS has a File menu")
    }

    #[test]
    fn every_action_appears_exactly_once() {
        let mut found = actions(&system_menu("macos"));
        found.sort_by_key(|action| format!("{action:?}"));
        assert_eq!(
            found,
            [
                MenuAction::About,
                MenuAction::Connect,
                MenuAction::Help,
                MenuAction::NewConnection,
                MenuAction::SaveConnection
            ]
        );
    }

    #[test]
    fn every_word_and_shortcut_is_one_a_listener_can_use() {
        for menu in system_menu("macos") {
            assert!(!menu.title.trim().is_empty());
            for item in menu.items {
                if let MenuItem::Acter {
                    label, accelerator, ..
                } = item
                {
                    assert!(!label.trim().is_empty(), "a menu item with no name");
                    if let Some(keys) = accelerator {
                        assert!(
                            keys.contains('+'),
                            "{keys} is one key, and on a Mac a lone function key needs fn"
                        );
                    }
                }
            }
        }
    }
}
