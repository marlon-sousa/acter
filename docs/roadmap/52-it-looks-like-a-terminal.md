# 52 — V1, to a sighted person, it looks like a terminal.

Lane: lane-5-look. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**V1, to a sighted person, it looks like a terminal.** Spec agreed in conversation on 2026-09-23 → implement.

Found on 2026-09-23 in a planning conversation about the 0.1 beta for Windows, by reading
`ui/src/styles.css` and `ui/src/views/main_window.html`. The stylesheet holds readable
defaults only, deliberately since A1, so a sighted person sees an unstyled web page: white
background, the system sans-serif font, thin black rules around the menu bar and status
bar, default buttons, plain dialog boxes, each command as a large bold heading and its
output as small monospace text. The user wants Acter to be beautiful without losing
accessibility, because it is also a portfolio project, and wants a sighted person to think
they are using a terminal.
