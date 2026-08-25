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
//! **Two members, and the second one waited for a port.** EOF is *shell* knowledge by
//! DESIGN's transport-versus-shell criterion — one transport, and every shell on it
//! answers differently — so B6 filed it against whichever entry first brought a shell that
//! needed it, and B5.2 is that entry. Interrupt, by the same criterion read the other way
//! round, is transport knowledge: fix the shell to bash and interrupting over WSL and over
//! SSH are still different mechanisms, which is why it is a
//! [`Transport`](crate::Transport) method (spec B6, decisions 5 and 6).
//!
//! What the port answers with turned out not to be a keystroke at all. B5.2 measured both
//! candidates against a real pseudoconsole and neither `0x1a` nor `0x04` ends a PowerShell
//! session; what ends one is the line `exit`. So the intent says what the user meant and
//! [`ShellAdapter::eof`](crate::ShellAdapter::eof) says what that costs in bytes, which is
//! exactly the split this enum exists for.
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
    /// Tell the far end there is no more input, which for a shell sitting at its prompt
    /// means end this session.
    ///
    /// Aimed at the far end rather than at a command, unlike [`Self::Interrupt`]: a
    /// program that reads standard input is entitled to it just as the shell is, and
    /// which of them is listening is not the domain's to know.
    Eof,
}
