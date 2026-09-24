# 54 — V3, the output carries the far end's colours.

Lane: lane-5-look. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**V3, the output carries the far end's colours.** Spec: none yet → specify first.

Found on 2026-09-23 in the conversation that agreed V1. DESIGN.md records that the terminal
grid carries attributes and the buffer does not, so every output row reaches the frontend as
plain text. A real terminal shows red errors, coloured `ls` listings and green and red in
`git diff`, and a sighted person notices their absence first. The user wants to choose the
palette in this entry.

**Direction from the user, 2026-09-23:** colour is not announced for now. It travels with the
text as metadata, so it can be drawn for a sighted person, and whether and how a screen
reader user ever hears it is decided later, with the colour already there to work from.
