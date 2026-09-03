//! Policy: what the operating system's own menu bar holds, if that operating system has
//! one Acter puts anything in.
//!
//! **A value, and the platform is an argument** — ARCHITECTURE's platform-divergence rule
//! in the mildest form it has, the same one [`offered`](crate::offered) uses for the
//! connect list since M1. Every platform's menu is compiled and asserted on every platform,
//! so a Windows machine can run the macOS assertions and a Mac can run Windows'.
//!
//! **An empty list means "not here", and that is what keeps Windows safe.** The composition
//! root attaches a native menu only when this answers with one, so the platform where a
//! native menu freezes NVDA for tens of seconds (spec A7) is not one line away from getting
//! one by accident — it is a platform this function says nothing for. Tauri would otherwise
//! install a default macOS menu of its own, which is what a Mac has today and what this
//! replaces (spec M3).
//!
//! # Who writes the words
//!
//! **Acter's own items are Acter's words**, because there is no platform answer to "what is
//! this application's Connect item called". **The platform's items keep the platform's
//! words**: Cut, Paste, Minimise and Quit are localised by macOS into the language the
//! account is set to — this project's own user runs a Brazilian Portuguese system — and
//! passing English through would replace a translation with a string nobody asked for.
//!
//! It costs one thing, measured 2026-09-02: an unbundled build's process name is
//! `acter-app`, so those items currently read "Quit acter-app" rather than "Quit Acter".
//! That is a fact about the *bundle*, and M4 is the entry that produces one — the name
//! comes right there, in every language, rather than by hard-coding one language here.

use crate::MenuAction;

/// One menu in the bar, in the order the bar lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemMenu {
    /// What the menu is called. The first one is the application menu, and macOS renders
    /// its title in bold as the application's own name.
    pub title: &'static str,
    /// What it holds, top to bottom.
    pub items: Vec<MenuItem>,
}

/// One line in a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    /// An item Acter answers itself: choosing it reaches the frontend as a [`MenuAction`].
    Acter {
        /// What choosing it asks for.
        action: MenuAction,
        /// What it is called, in Acter's words.
        label: &'static str,
        /// The keystroke that runs it without opening the menu, or `None` for an item that
        /// has no shortcut. **Spelled the way the menu library parses it**, and never a
        /// function key: on a Mac with factory settings the function keys need `fn`
        /// (measured 2026-09-02), so an item whose only shortcut was one would have a
        /// shortcut most listeners cannot press.
        accelerator: Option<&'static str>,
    },
    /// An item the platform owns, including its words and its conventional shortcut.
    Standard(Standard),
    /// A rule between groups. It is not focusable and a reader passes over it, so it
    /// costs a listener nothing and tells a sighted user where a group ends.
    Separator,
}

/// The items every application on the platform has, done by the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Standard {
    /// The system's Services submenu.
    Services,
    /// Hide this application.
    Hide,
    /// Hide every other application.
    HideOthers,
    /// Show everything that was hidden.
    ShowAll,
    /// End the application. **Measured 2026-09-02**: it takes the running shell with it,
    /// which is why Acter lets the platform own its quit (spec M3, decision 3).
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    /// Close this window, which for Acter ends the application (measured 2026-09-02).
    CloseWindow,
    Minimize,
    /// macOS's Zoom.
    Maximize,
    /// Toggle full screen.
    Fullscreen,
}

/// What this operating system's menu bar holds, or nothing if its menu is not there.
///
/// Windows answers with nothing because its menu bar is in the document (spec A7), Linux
/// because nobody has decided yet and an empty answer is the honest one.
pub fn system_menu(os: &str) -> Vec<SystemMenu> {
    match os {
        "macos" => macos(),
        _ => Vec::new(),
    }
}

/// **Six menus, which is more than the two Windows earned, and deliberately so.** DESIGN's
/// rule is that a menu has to earn its place, and on macOS the room is different: the
/// system augments a bar it recognises — the Window menu collects window commands, Help
/// gets the system's own help search — so a bar missing what every other application has is
/// its own kind of surprise (spec M3, decision 2).
fn macos() -> Vec<SystemMenu> {
    vec![
        SystemMenu {
            title: "Acter",
            items: vec![
                // **Acter's own dialog rather than the system's About panel** (spec M3,
                // decision 3): it reads name, version, copyright and licence, and the
                // native panel would have only the first two until a bundle carries the
                // rest.
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
                // **The item this entry exists for.** Connect is in no macOS menu today, so
                // the one control the product is about is reachable only from the button in
                // the unconnected window — and not at all once a session is running.
                //
                // **Cmd+K is the platform's own "connect to server"**, which is what a Mac
                // user's hands already do.
                MenuItem::Acter {
                    action: MenuAction::Connect,
                    label: "Connect…",
                    accelerator: Some("CmdOrCtrl+K"),
                },
                MenuItem::Separator,
                MenuItem::Standard(Standard::CloseWindow),
            ],
        },
        // **Edit is not decoration on this platform.** macOS routes a webview's own copy and
        // paste through the menu bar, so a bar without these items takes Cmd+C out of the
        // command line — which on a terminal for listeners is not a cosmetic loss.
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
        // **The submenu that is empty today**, measured on 2026-09-02: Tauri's default menu
        // builds a Help menu and puts nothing in it on macOS, so a listener who opens it
        // arrives somewhere that says nothing.
        //
        // **Cmd+slash rather than the Cmd+question mark every Mac application uses**, and
        // that is measured rather than preferred: macOS reserves ⇧⌘/ for the search field it
        // injects into any menu called Help, and it wins. With the item bound to it, pressing
        // it put focus in that search field and Acter's help never opened — a shortcut the
        // menu advertises and does not run. ⌘/ is free, reaches the application and opens the
        // topic. F1 keeps working and is unchanged.
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

    /// The platform where a native menu freezes the reader asks for none, and that is
    /// asserted rather than left to the composition root's `if` (spec A7).
    #[test]
    fn windows_asks_for_no_native_menu_because_its_menu_is_in_the_document() {
        assert!(system_menu("windows").is_empty());
    }

    #[test]
    fn a_platform_with_no_answer_yet_says_nothing_rather_than_guessing() {
        assert!(system_menu("linux").is_empty());
        assert!(system_menu("freebsd").is_empty());
    }

    /// Runs on Windows CI as well as on a Mac, which is the whole point of the platform
    /// being an argument.
    #[test]
    fn macos_has_the_six_menus_the_platform_expects() {
        let titles: Vec<_> = system_menu("macos").iter().map(|menu| menu.title).collect();
        assert_eq!(titles, ["Acter", "File", "Edit", "View", "Window", "Help"]);
    }

    /// The defect this entry was written to fix, asserted as a defect: the menu a listener
    /// opens looking for help must have something in it.
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

    /// Connect is reachable from the menu bar whatever the window is showing, which it is
    /// not today: the button exists only in the unconnected window.
    #[test]
    fn connect_is_in_the_menu_and_is_in_file() {
        let file = system_menu("macos")
            .into_iter()
            .find(|menu| menu.title == "File")
            .expect("macOS has a File menu");
        assert!(matches!(
            file.items.first(),
            Some(MenuItem::Acter {
                action: MenuAction::Connect,
                ..
            })
        ));
    }

    /// Every variant is reachable, and none twice: an action with no item cannot be chosen,
    /// and an action in two places is two ways to say one thing in a bar a listener arrows
    /// through.
    #[test]
    fn every_action_appears_exactly_once() {
        let mut found = actions(&system_menu("macos"));
        found.sort_by_key(|action| format!("{action:?}"));
        assert_eq!(
            found,
            [MenuAction::About, MenuAction::Connect, MenuAction::Help]
        );
    }

    /// Every string here is spoken, so none of them is empty and none is a function key
    /// standing alone — on a Mac with factory settings those need `fn`.
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
