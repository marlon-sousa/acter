# 45 — A full reset leaves Acter believing the alternate screen is up.

Lane: lane-2-domain. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**A full reset leaves Acter believing the alternate screen is up.** Spec: none yet →
specify first.

Found on 2026-09-23 while stripping comments for C9 (PR #69), by reading the code and the
library source; not yet reproduced in the running application.

`ESC c` (RIS, a full reset) is what the `reset` command sends, and a program can send it
from the alternate screen. In alacritty_terminal 0.26.0, `Term::reset_state`
(`src/term/mod.rs`, around line 1835) swaps the grids back when `TermMode::ALT_SCREEN` is
set: `mem::swap(&mut self.grid, &mut self.inactive_grid)`. It does this directly, not
through `unset_private_mode`.

Acter learns about screen switches only from its own sniffer
(`crates/acter-term/src/alacritty_engine/sniffer.rs`), which implements
`set_private_mode` and `unset_private_mode` and signals `ScreenChanged` only for mode 1049
(`swaps_screen`). It has no reset handler. So after `ESC c` on the alternate screen, the
emulator is back on the normal grid, no `ScreenChanged(Normal)` is emitted, and
`SessionState` keeps `Screen::Alternate`: no `AltScreenLeft` reaches the frontend. The
comment on `swaps_screen` used to claim the sniffer and the emulator could never disagree
about which screen is current.
