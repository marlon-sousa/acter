//! Policy: what the connect list shows — which kinds an operating system offers, in what
//! order, and how an entry reads when this machine cannot start it.

use crate::ConnectionKind;

/// One row of the connect list, as the dialog will render it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub kind: ConnectionKind,
    /// Includes the `(not available)` suffix when the row cannot be chosen.
    pub label: String,
    pub available: bool,
}

impl Connection {
    /// `None` when the row is available.
    pub fn instructions(&self) -> Option<&'static str> {
        if self.available {
            None
        } else {
            Some(self.kind.instructions())
        }
    }
}

const NOT_AVAILABLE: &str = " (not available)";

/// PowerShell's editions and WSL's distributions are not entries here; they are chosen in
/// the connect dialog's panel.
const ON_WINDOWS: &[ConnectionKind] = &[
    ConnectionKind::Cmd,
    ConnectionKind::PowerShell,
    ConnectionKind::Wsl,
    ConnectionKind::Ssh,
];

/// What macOS offers: a shell on this Mac, and a machine that is not this one.
///
/// This Mac's shells are not entries either, for the same reason as [`ON_WINDOWS`]'s
/// editions and distributions.
///
/// Windows-only kinds are absent from this list rather than listed as unavailable.
const ON_MACOS: &[ConnectionKind] = &[ConnectionKind::Terminal, ConnectionKind::Ssh];

/// `os` is the spelling `std::env::consts::OS` uses ("windows", "macos"), the same string
/// `routers::platform` gives the frontend.
pub fn offered(os: &str) -> &'static [ConnectionKind] {
    match os {
        "windows" => ON_WINDOWS,
        "macos" => ON_MACOS,
        _ => &[],
    }
}

/// The connect list: every kind offered, available ones first, each labelled.
///
/// Preserves each group's relative order rather than sorting; only the availability
/// boundary moves.
pub fn catalogue(
    kinds: &[ConnectionKind],
    has: impl Fn(ConnectionKind) -> bool,
) -> Vec<Connection> {
    let (available, missing): (Vec<ConnectionKind>, Vec<ConnectionKind>) =
        kinds.iter().copied().partition(|kind| has(*kind));

    available
        .into_iter()
        .map(|kind| Connection {
            kind,
            label: kind.label().to_owned(),
            available: true,
        })
        .chain(missing.into_iter().map(|kind| Connection {
            kind,
            label: format!("{}{NOT_AVAILABLE}", kind.label()),
            available: false,
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_machine_with_everything_lists_everything_in_order() {
        for os in ["windows", "macos"] {
            let kinds = offered(os);
            let listed = catalogue(kinds, |_| true);

            assert_eq!(
                listed
                    .iter()
                    .map(|row| row.label.as_str())
                    .collect::<Vec<_>>(),
                kinds.iter().map(|kind| kind.label()).collect::<Vec<_>>(),
                "{os} lists its own kinds in its own order"
            );
            assert!(listed.iter().all(|row| row.available));
            assert!(
                listed.iter().all(|row| row.instructions().is_none()),
                "an available row has no instructions to arrow past"
            );
        }
    }

    #[test]
    fn a_missing_kind_goes_to_the_end_and_says_so_in_its_name() {
        let listed = catalogue(offered("windows"), |kind| kind != ConnectionKind::Wsl);

        let last = listed.last().expect("the list is not empty");
        assert_eq!(last.kind, ConnectionKind::Wsl);
        assert_eq!(last.label, "WSL (not available)");
        assert!(!last.available);
        assert!(
            last.instructions()
                .expect("a missing kind explains itself")
                .contains("wsl --install"),
            "and says what to type about it"
        );
    }

    #[test]
    fn the_list_is_the_same_length_however_little_is_installed() {
        for os in ["windows", "macos"] {
            let kinds = offered(os);

            assert_eq!(catalogue(kinds, |_| true).len(), kinds.len(), "{os}");
            assert_eq!(catalogue(kinds, |_| false).len(), kinds.len(), "{os}");
        }
    }

    #[test]
    fn installing_something_moves_one_row_rather_than_reordering_the_list() {
        let windows = offered("windows");
        let before = catalogue(windows, |kind| {
            !matches!(kind, ConnectionKind::PowerShell | ConnectionKind::Wsl)
        });
        let after = catalogue(windows, |kind| kind != ConnectionKind::Wsl);

        assert_eq!(
            before.iter().map(|row| row.kind).collect::<Vec<_>>(),
            [
                ConnectionKind::Cmd,
                ConnectionKind::Ssh,
                ConnectionKind::PowerShell,
                ConnectionKind::Wsl,
            ],
            "PowerShell missing sorts it beside WSL, both after what works"
        );
        assert_eq!(
            after.iter().map(|row| row.kind).collect::<Vec<_>>(),
            [
                ConnectionKind::Cmd,
                ConnectionKind::PowerShell,
                ConnectionKind::Ssh,
                ConnectionKind::Wsl,
            ],
            "and installing it changes only whether it is available, not where it is"
        );
        let powershell = |rows: &[Connection]| {
            rows.iter()
                .find(|row| row.kind == ConnectionKind::PowerShell)
                .expect("PowerShell is listed either way")
                .available
        };
        assert!(!powershell(&before) && powershell(&after));
    }

    #[test]
    fn an_edition_is_never_a_row_of_its_own() {
        let listed = catalogue(offered("windows"), |_| true);

        for edition in ConnectionKind::PowerShell.editions() {
            assert!(
                !listed.iter().any(|row| row.kind == *edition),
                "{edition:?} is a variant of PowerShell, not a row"
            );
        }
        assert!(
            listed
                .iter()
                .any(|row| row.kind == ConnectionKind::PowerShell)
        );
    }

    #[test]
    fn each_platform_offers_its_own_kinds() {
        assert_eq!(
            offered("windows"),
            [
                ConnectionKind::Cmd,
                ConnectionKind::PowerShell,
                ConnectionKind::Wsl,
                ConnectionKind::Ssh,
            ],
            "Windows offers its own shells and SSH"
        );
        assert_eq!(
            offered("macos"),
            [ConnectionKind::Terminal, ConnectionKind::Ssh],
            "macOS offers a shell on this Mac, then a machine that is not this one"
        );
    }

    #[test]
    fn a_windows_kind_is_absent_from_macos_rather_than_offered_as_missing() {
        let listed = catalogue(offered("macos"), |_| false);

        for windows_only in [
            ConnectionKind::Cmd,
            ConnectionKind::PowerShell,
            ConnectionKind::Wsl,
        ] {
            assert!(
                !listed.iter().any(|row| row.kind == windows_only),
                "{windows_only:?} is not something a Mac can be missing"
            );
        }
    }

    #[test]
    fn ssh_is_offered_by_every_platform_that_is_offered_anything() {
        for os in ["windows", "macos"] {
            assert!(
                offered(os).contains(&ConnectionKind::Ssh),
                "{os} can reach a far end that is not on this machine"
            );
        }
    }

    #[test]
    fn a_platform_with_no_kinds_yet_offers_nothing_rather_than_another_platforms() {
        for os in ["linux", "freebsd", "android"] {
            assert!(offered(os).is_empty(), "{os} has not been built for");
            assert!(catalogue(offered(os), |_| true).is_empty());
        }
    }
}
