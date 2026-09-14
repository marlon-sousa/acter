# 38 — The set-up dialog reads its whole command aloud on macOS.

Lane: lane-3-macos. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**The set-up dialog reads its whole command aloud on macOS.** Spec: none yet → specify
first. **Measured 2026-09-03**, opening a local `bash` session: the dialog announced
itself and then read the setup command escape by escape — around forty utterances of
`printf backslash 033]133;C backslash 033 backslash backslash`, `underscore underscore
acter underscore started`, and so on — and then read it a second time. Fifteen seconds of
shell escapes before anything else can be done, on a dialog whose question is one sentence
long.

**The command is deliberately walkable character by character** (spec B9.5), and that is
not in question; what is, is a reader that reads all of it on arrival without being asked.
The first step is to find out whether VoiceOver is reading the dialog's whole contents on
open or whether the command's own markup asks for it, and the answer decides whether this
is a `data-platform` matter or a change to the dialog for everyone.
