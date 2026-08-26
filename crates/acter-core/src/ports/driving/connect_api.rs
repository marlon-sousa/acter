//! Port (driving): what may be asked about connecting — what this machine offers, which
//! far end is behind the window now, and starting one in place of another.
//!
//! **Actions rather than a user interface, and that is the whole shape of B7.** No suite in
//! this project can drive a menu: `MockRuntime` does not run the native webview and
//! WebDriver drives only the webview, so a design where "connecting" lives inside a menu
//! handler is a design where connecting is untested. Naming the two operations puts the
//! behaviour behind a seam a test can reach with no window, no webview and no screen reader
//! in the way, and leaves the menu as the thinnest possible caller of them — which is also
//! what the launch path calls when `--profile` names one (B8).
//!
//! Separate from [`SessionApi`](crate::SessionApi) because it is a different conversation:
//! that port is one session's input and output, and this one is about *which* session there
//! is. A caller that could only reach the first has no way to ask for another.
//!
//! Sync and dyn-compatible, per ARCHITECTURE's rule for port traits. Starting a shell is
//! I/O, and it happens behind [`SessionFactory`](crate::SessionFactory) — the invoke that
//! calls `use_profile` waits only as long as spawning a process takes, never on the shell
//! saying anything, which is what `ConnectionState::Connecting` exists to cover.

use crate::{Connectable, Connected, ProfileId};

/// Connecting, as two actions and a question.
pub trait ConnectApi: Send + Sync {
    /// Everything this machine offers, freshly asked each time.
    ///
    /// **Never cached** (spec B7, decision 6). Distributions and PowerShell editions get
    /// installed while an application is running, and a list computed once at startup would
    /// be quietly wrong with no way for the user to notice — they would have to restart
    /// Acter to be told the truth about their own machine.
    ///
    /// Includes what this machine *cannot* start, labelled and last, because a list that
    /// silently omits WSL teaches a listener that Acter does not support it (spec B5.4).
    fn connectable(&self) -> Vec<Connectable>;

    /// Start this one and replace whatever was running, or say — in a sentence a listener
    /// can hear — why it could not be started.
    ///
    /// **The new session is built before the old one is dropped** (spec B7, decision 5), so
    /// a failure costs the user nothing: not their place, not their history, not even the
    /// shell they were in. There is a person standing in front of this with a working
    /// session behind them, and a speakable sentence is the only acceptable answer.
    ///
    /// The caller then attaches to the returned [`Connected::session`]. That attach is
    /// explicit rather than a second effect of this call, so the frontend can clear its
    /// buffer at a moment it chooses and nothing lands in a buffer still showing another
    /// shell's output (spec B7, decision 1).
    fn use_profile(&self, id: &ProfileId) -> Result<Connected, String>;

    /// Which far end is behind the window now, or `None` for a window connected to
    /// nothing.
    ///
    /// Asked once at startup, because a launch may or may not have brought a session with
    /// it, and the two are different windows to open: one attaches, the other says it is
    /// empty and where to go.
    fn connected(&self) -> Option<Connected>;
}
