//! Entity/value: what a keystroke means to a session, once the keybinding policy has
//! read a [`KeyPress`](crate::KeyPress).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIntent {
    /// Carries no command id; the service knows which command is running.
    Interrupt,
    /// End of input to the far end, which ends the session if the shell is at its prompt.
    Eof,
}
