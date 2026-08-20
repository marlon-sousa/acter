//! Entity/value: what a keystroke *means* to a session — the domain's own vocabulary,
//! on the far side of the keybinding policy from [`KeyPress`](crate::KeyPress).
//!
//! Two vocabularies rather than one because the cut between the frontend and the domain
//! falls at the key, not at the meaning (spec B6, decision 4): the frontend reports what
//! was pressed, `policies::keybindings` decides what it means, and this is what comes
//! out. That separation is what lets bindings become configuration later without a
//! frontend release, and it is already required by a Decided binding —
//! `Ctrl+Shift+Space` means "send the next keystroke literally", which is by definition
//! not expressible as an intent.
//!
//! **One member, deliberately.** EOF is the obvious second and is not here: by DESIGN's
//! own transport-versus-shell criterion it is *shell* knowledge — PowerShell on ConPTY
//! wants Ctrl+Z, bash over WSL wants `0x04`, same transport and different answers — so
//! it arrives with `ShellAdapter` in B5. Interrupt, by the same criterion, is transport
//! knowledge: fix the shell to bash and interrupting over WSL and over SSH are still
//! different mechanisms, which is why it is a
//! [`Transport`](crate::Transport) method (spec B6, decisions 5 and 6).
//!
//! Not a protocol type: nothing on the wire carries one. It exists between the policy
//! and the service, both of which are in this crate.

/// Something a session can be asked to do, arrived at from a keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIntent {
    /// Stop whatever command is running. Which one is the service's to know: a
    /// frontend-supplied id could only be stale, since the command may have ended
    /// between the keypress and the invoke (spec B6, decision 7).
    Interrupt,
}
