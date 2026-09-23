//! Adapter: the fake far end of a scripted session — what a shell says, and how its
//! bytes arrive.

mod chunking;
mod shell;
mod transcript_shell;
mod unmarked;

pub use chunking::Chunking;
pub use shell::{FakeShell, Script, Submission};
pub use transcript_shell::TranscriptShell;
pub use unmarked::Unmarked;
