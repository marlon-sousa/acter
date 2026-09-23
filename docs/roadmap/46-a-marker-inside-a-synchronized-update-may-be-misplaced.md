# 46 — A marker inside a synchronized update may be placed before the output ahead of it.

Lane: lane-2-domain. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**A marker inside a synchronized update may be placed before the output ahead of it.**
Spec: none yet → specify first. Suspected from reading the code on 2026-09-23 while
stripping comments for C9 (PR #69); not yet reproduced by a test or a real program.

`crates/acter-term/src/alacritty_engine.rs` runs the same bytes through two vte parsers:
the sniffer, which finds OSC 133 markers and screen switches, and alacritty's `Term`, which
draws the grid. When the sniffer signals, the engine places the signal against the grid as
it stands. Outside a synchronized update, the marker's own bytes draw nothing, so the grid
is in the state the marker belongs to.

In vte 0.15 (the fork Acter builds against, `src/ansi.rs`), a synchronized update is
`CSI ? 2026 h` … `CSI ? 2026 l`, and while one is open the parser buffers every byte. A
buffered update is flushed only by the closing `ESC[?2026l`, by `stop_sync`, which Acter
never calls, or when the 2 MiB buffer would overflow. `StdSyncHandler::pending_timeout` is
just `timeout.is_some()`, so the timeout never fires on its own. Inside an update, the
sniffer therefore signals only when the closing sequence arrives. If an update contains
output followed by an OSC 133 `D`, the question is whether the `D` is placed before that
output reaches the grid.

The overflow check depends on slice size: the sniffer is fed one byte at a time and the
`Term` whole segments, so the two parsers can also flush at different bytes.

`docs/specs/b3-terminal-engine.md`, around line 74, describes a "DCS 2026 timeout" per
parser. Both the sequence and the timeout are wrong for vte 0.15.
