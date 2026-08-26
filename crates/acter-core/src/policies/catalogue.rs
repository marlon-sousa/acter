//! Policy: what the connect list shows — which kinds belong to this platform, in what
//! order, and how an entry reads when this machine cannot start it.
//!
//! **A pure function from what the machine has to what the listener hears.** Finding out
//! whether PowerShell 7 is installed is I/O and belongs behind `InstalledShells` with B7;
//! this decides what to *do* with that answer, which is why every rule here is testable
//! without a machine that happens to be missing something (spec B5.4, decision 6).
//!
//! **Two absences, kept apart.** A kind that does not belong to this operating system is not
//! in the catalogue at all — offering `WSL (not available)` on macOS, with instructions to
//! install Windows, would be absurd. A kind that belongs here and is missing *is* in the
//! catalogue: last, labelled, and with something to say about itself.

use crate::ConnectionKind;

/// One row of the connect list, as the dialog will render it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// Which kind this row offers.
    pub kind: ConnectionKind,
    /// What the row is called, including `(not available)` when it is not.
    pub label: String,
    /// Whether choosing it can start a session.
    pub available: bool,
}

impl Connection {
    /// What to say about a row that cannot be connected to. `None` when it can, because a
    /// panel of instructions under an available kind is noise a listener has to arrow past.
    pub fn instructions(&self) -> Option<&'static str> {
        if self.available {
            None
        } else {
            Some(self.kind.instructions())
        }
    }
}

/// The suffix an unavailable row carries, in its **name** rather than in a visual state.
///
/// A greyed-out row that looks different and reads the same is precisely the failure this
/// product exists to avoid, so the words are part of what a screen reader announces
/// (spec B5.4, decision 2).
const NOT_AVAILABLE: &str = " (not available)";

/// The kinds this build offers at all, in the order they are listed when everything is
/// available.
///
/// cmd first because it is the one that is always there; then PowerShell; then WSL, which is
/// the heaviest thing to have installed and the least likely to be somebody's daily shell.
///
/// **The two PowerShell editions are not here**, and neither are WSL's distributions: both
/// are *variants* of a kind, chosen in the connect dialog's panel rather than arrowed as
/// rows of their own (spec A11). A listener meets three kinds however many editions and
/// distributions this machine happens to have.
#[cfg(windows)]
const ON_THIS_PLATFORM: &[ConnectionKind] = &[
    ConnectionKind::Cmd,
    ConnectionKind::PowerShell,
    ConnectionKind::Wsl,
];

/// No kind is built for any other platform yet. A Linux terminal and a macOS terminal become
/// entries of their own when those builds exist, rather than these ones pretending to be
/// available there.
#[cfg(not(windows))]
const ON_THIS_PLATFORM: &[ConnectionKind] = &[];

/// The connect list: every kind this platform has, available ones first, each labelled.
///
/// `has` answers whether a kind can be started on this machine — B7 asks the machine; a test
/// answers for itself, which is the point of taking it as an argument.
///
/// **The order within each group is preserved rather than sorted**, so the list a user
/// learns does not rearrange itself when they install something. Only the boundary between
/// available and unavailable moves.
pub fn catalogue(has: impl Fn(ConnectionKind) -> bool) -> Vec<Connection> {
    let (available, missing): (Vec<ConnectionKind>, Vec<ConnectionKind>) = ON_THIS_PLATFORM
        .iter()
        .copied()
        .partition(|kind| has(*kind));

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

    /// Everything present: the list is the platform's own order, and nothing is labelled.
    #[test]
    fn a_machine_with_everything_lists_everything_in_order() {
        let listed = catalogue(|_| true);

        assert_eq!(
            listed
                .iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            ON_THIS_PLATFORM
                .iter()
                .map(|kind| kind.label())
                .collect::<Vec<_>>()
        );
        assert!(listed.iter().all(|row| row.available));
        assert!(
            listed.iter().all(|row| row.instructions().is_none()),
            "an available row has no instructions to arrow past"
        );
    }

    /// **The rule the whole entry is about.** A missing kind is still offered, still
    /// readable, and last.
    #[cfg(windows)]
    #[test]
    fn a_missing_kind_goes_to_the_end_and_says_so_in_its_name() {
        let listed = catalogue(|kind| kind != ConnectionKind::Wsl);

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

    /// Nothing is hidden, whatever the machine has. A list whose *length* changes with the
    /// machine cannot be learned by somebody navigating it by ear.
    #[test]
    fn the_list_is_the_same_length_however_little_is_installed() {
        assert_eq!(catalogue(|_| true).len(), catalogue(|_| false).len());
        assert_eq!(catalogue(|_| false).len(), ON_THIS_PLATFORM.len());
    }

    /// The order *within* each group survives, so installing something moves one row across
    /// the boundary rather than rearranging the list somebody has learned.
    #[cfg(windows)]
    #[test]
    fn installing_something_moves_one_row_rather_than_reordering_the_list() {
        let order = [
            ConnectionKind::Cmd,
            ConnectionKind::PowerShell,
            ConnectionKind::Wsl,
        ];
        let before =
            catalogue(|kind| !matches!(kind, ConnectionKind::PowerShell | ConnectionKind::Wsl));
        let after = catalogue(|kind| kind != ConnectionKind::Wsl);

        assert_eq!(
            before.iter().map(|row| row.kind).collect::<Vec<_>>(),
            order,
            "PowerShell missing sorts it beside WSL, both after what works"
        );
        assert_eq!(
            after.iter().map(|row| row.kind).collect::<Vec<_>>(),
            order,
            "and installing it changes only whether it is available, not where it is"
        );
        assert!(!before[1].available && after[1].available);
    }

    /// **The editions and the distributions are not rows** (spec A11). A listener meets
    /// three kinds however many of either this machine has, and chooses between them in the
    /// dialog's panel.
    #[cfg(windows)]
    #[test]
    fn an_edition_is_never_a_row_of_its_own() {
        let listed = catalogue(|_| true);

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

    /// Platform gating, asserted for the build this runs on rather than in the abstract.
    #[cfg(windows)]
    #[test]
    fn a_windows_build_offers_the_windows_kinds() {
        let listed = catalogue(|_| true);

        assert_eq!(listed.len(), 3);
        assert!(listed.iter().any(|row| row.kind == ConnectionKind::Cmd));
        assert!(listed.iter().any(|row| row.kind == ConnectionKind::Wsl));
    }

    /// And the other half of decision 5: a platform with no kinds built for it offers
    /// nothing at all, rather than offering Windows shells that could never work there.
    #[cfg(not(windows))]
    #[test]
    fn a_platform_with_no_kinds_yet_offers_nothing_rather_than_windows_ones() {
        assert!(catalogue(|_| true).is_empty());
    }
}
