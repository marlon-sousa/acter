//! Entity/value: what a keystroke *means* to a session — the domain's own vocabulary,
//! on the far side of the keybinding policy from [`KeyPress`](crate::KeyPress).
//!
//! Not a protocol type: nothing on the wire carries one. It exists between the policy
//! and the service, both of which are in this crate.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIntent {
    /// Which command is the service's to know: a frontend-supplied id could only be
    /// stale, since the command may have ended between the keypress and the invoke.
    Interrupt,
    /// Tell the far end there is no more input, which for a shell at its prompt means
    /// end this session.
    ///
    /// Aimed at the far end rather than at a command, unlike [`Self::Interrupt`]: which
    /// of a listening program or the shell is entitled to it is not the domain's to know.
    Eof,
}
