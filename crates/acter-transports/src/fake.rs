//! Adapter: the fake far end of a scripted session — what a shell says, and how its
//! bytes arrive.
//!
//! Until B3.6 those two were one thing. `ScriptedTransport` drew the prompt, echoed the
//! command line, matched the rule *and* decided where each read ended, so a transcript
//! had to be authored for a particular chunking and DESIGN's "a marker split across two
//! reads" was proven for exactly one hand-written marker. Splitting them means they
//! compose: any transcript, crossed with any far-end trait, cut any way.
//!
//! - [`FakeShell`] is the seam, and [`Script`] is what crosses it.
//! - [`TranscriptShell`] answers from a `SessionTranscript`; [`Unmarked`] wraps any
//!   shell and drops its shell-integration markers.
//! - [`Chunking`] is the pipe's half: how one delivery becomes reads.
//!
//! Deliberately *not* in `acter-shells`. That crate's theme is shell knowledge the
//! domain calls for — injection snippets, quoting rules, completion strategy — behind
//! the `ShellAdapter` port. A fake far end is not that: the domain never calls it, it
//! sits on the far side of the transport seam rather than beside it, and putting it
//! there would make `acter-transports` depend on `acter-shells` for nothing (spec B3.6,
//! decision 6).
//!
//! Facade: this file only declares modules and re-exports.

mod chunking;
mod shell;
mod transcript_shell;
mod unmarked;

pub use chunking::Chunking;
pub use shell::{FakeShell, Script, Submission};
pub use transcript_shell::TranscriptShell;
pub use unmarked::Unmarked;
