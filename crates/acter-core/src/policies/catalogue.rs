//! Policy: what the connect list shows — which kinds an operating system offers, in what
//! order, and how an entry reads when this machine cannot start it.
//!
//! **A pure function from what the machine has to what the listener hears.** Finding out
//! whether PowerShell 7 is installed is I/O and belongs behind `ThisComputer` with B7;
//! this decides what to *do* with that answer, which is why every rule here is testable
//! without a machine that happens to be missing something (spec B5.4, decision 6).
//!
//! **Two absences, kept apart.** A kind that does not belong to this operating system is not
//! in the catalogue at all — offering `WSL (not available)` on macOS, with instructions to
//! install Windows, would be absurd. A kind that belongs here and is missing *is* in the
//! catalogue: last, labelled, and with something to say about itself.
//!
//! # The platform is an argument since M1, not a `#[cfg]`
//!
//! Both lists used to be `#[cfg]`-selected constants, and only one of them existed in any
//! given build. That made the macOS behaviour testable only on a Mac and the Windows
//! behaviour only on Windows — so whichever platform a developer changed this on, half of
//! what they changed was unasserted. It also cost `connect`'s own tests a failure that had
//! nothing to do with connecting: with an empty list off Windows, a service that asks the
//! machine on every call never asked it at all.
//!
//! So [`offered`] takes the operating system's name and every platform's answer is compiled,
//! asserted and readable everywhere. This is ARCHITECTURE's platform-divergence rule in its
//! mildest form: the answer is a value, so it needs no gate, no adapter and no port.

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

/// What Windows offers, in the order they are listed when everything is available.
///
/// cmd first because it is the one that is always there; then PowerShell; then WSL, which is
/// the heaviest thing to have installed and the least likely to be somebody's daily shell.
///
/// **The two PowerShell editions are not here**, and neither are WSL's distributions: both
/// are *variants* of a kind, chosen in the connect dialog's panel rather than arrowed as
/// rows of their own (spec A11). A listener meets three kinds however many editions and
/// distributions this machine happens to have.
const ON_WINDOWS: &[ConnectionKind] = &[
    ConnectionKind::Cmd,
    ConnectionKind::PowerShell,
    ConnectionKind::Wsl,
    // Last, because it is the one that needs a form filled in rather than a choice made,
    // and because a listener arrowing the list meets their own machine's shells first.
    ConnectionKind::Ssh,
];

/// What macOS offers: a shell on this Mac, and a machine that is not this one.
///
/// **Terminal first, for the reason cmd comes first on Windows** — a listener arrowing the
/// list meets their own machine before a form to fill in. It arrived with M2; M1 shipped
/// this list with SSH alone, because Acter speaks SSH itself rather than running a client
/// (spec B9, decision 1), so the kind that would have been hardest to port needed no porting
/// at all.
///
/// **The shells this Mac has are not here**, any more than PowerShell's editions or WSL's
/// distributions are: they are *variants* of Terminal, chosen in the dialog's panel (DESIGN,
/// decided 2026-08-31). A listener meets two kinds however many shells `/etc/shells` names.
///
/// cmd, PowerShell and WSL are absent rather than unavailable — a Mac told to install
/// Windows is the absurdity [`NOT_AVAILABLE`] exists to avoid where it means something.
const ON_MACOS: &[ConnectionKind] = &[ConnectionKind::Terminal, ConnectionKind::Ssh];

/// The kinds this operating system offers, in the order a listener meets them.
///
/// Takes the name `std::env::consts::OS` uses, which is what the composition root hands it
/// and what `routers::platform` already answers the frontend with — one spelling of "which
/// platform is this" in the whole product rather than two that can disagree.
///
/// **An operating system nobody has built for offers nothing**, rather than offering Windows
/// shells that could never start there. That is not a degradation: it is the honest answer
/// until somebody does the work, and the connect dialog says so with an empty list rather
/// than with four rows of instructions for a different computer.
pub fn offered(os: &str) -> &'static [ConnectionKind] {
    match os {
        "windows" => ON_WINDOWS,
        "macos" => ON_MACOS,
        _ => &[],
    }
}

/// The connect list: every kind offered, available ones first, each labelled.
///
/// `kinds` is what [`offered`] answered for this build, passed in rather than read here so
/// that a caller's tests can ask for any platform's list on any machine.
///
/// `has` answers whether a kind can be started on this machine — B7 asks the machine; a test
/// answers for itself, which is the point of taking it as an argument.
///
/// **The order within each group is preserved rather than sorted**, so the list a user
/// learns does not rearrange itself when they install something. Only the boundary between
/// available and unavailable moves.
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

    /// Everything present: the list is the platform's own order, and nothing is labelled.
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

    /// **The rule the whole entry is about.** A missing kind is still offered, still
    /// readable, and last.
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

    /// Nothing is hidden, whatever the machine has. A list whose *length* changes with the
    /// machine cannot be learned by somebody navigating it by ear.
    #[test]
    fn the_list_is_the_same_length_however_little_is_installed() {
        for os in ["windows", "macos"] {
            let kinds = offered(os);

            assert_eq!(catalogue(kinds, |_| true).len(), kinds.len(), "{os}");
            assert_eq!(catalogue(kinds, |_| false).len(), kinds.len(), "{os}");
        }
    }

    /// The order *within* each group survives, so installing something moves one row across
    /// the boundary rather than rearranging the list somebody has learned.
    #[test]
    fn installing_something_moves_one_row_rather_than_reordering_the_list() {
        // **SSH is in both lists and never moves relative to what is available**, because
        // it cannot be missing: it is not a program on this machine (spec B9, decision 1).
        // What moves is PowerShell, from the missing group to the available one — which is
        // the whole subject of this test.
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
        // Found by kind rather than by position, which is the point being made: what
        // changed about PowerShell is whether it is available, and everything else stayed
        // where the user had learned to find it.
        let powershell = |rows: &[Connection]| {
            rows.iter()
                .find(|row| row.kind == ConnectionKind::PowerShell)
                .expect("PowerShell is listed either way")
                .available
        };
        assert!(!powershell(&before) && powershell(&after));
    }

    /// **The editions and the distributions are not rows** (spec A11). A listener meets
    /// three kinds however many of either this machine has, and chooses between them in the
    /// dialog's panel.
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

    /// **Platform gating, asserted for every platform from any machine** — which is the
    /// whole reason these lists stopped being `#[cfg]`-selected. Before M1 this test could
    /// only ever check the build it happened to be compiled into.
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

    /// **The Windows kinds are absent from macOS rather than unavailable on it** — the
    /// distinction this module's own doc comment is built on. A Mac offered
    /// `WSL (not available)` would be read instructions to install Windows.
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

    /// **SSH is the kind both platforms have**, and it is what makes the first macOS build
    /// worth running: nothing about it is Windows', so it needed no adapter to arrive here.
    #[test]
    fn ssh_is_offered_by_every_platform_that_is_offered_anything() {
        for os in ["windows", "macos"] {
            assert!(
                offered(os).contains(&ConnectionKind::Ssh),
                "{os} can reach a far end that is not on this machine"
            );
        }
    }

    /// And the other half: a platform nobody has built for offers nothing at all, rather
    /// than offering shells that could never start there.
    #[test]
    fn a_platform_with_no_kinds_yet_offers_nothing_rather_than_another_platforms() {
        for os in ["linux", "freebsd", "android"] {
            assert!(offered(os).is_empty(), "{os} has not been built for");
            assert!(catalogue(offered(os), |_| true).is_empty());
        }
    }
}
