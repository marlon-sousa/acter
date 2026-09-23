# 50 — The far-end toggle depends on the keyboard layout.

Lane: lane-1-ui. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**The far-end toggle depends on the keyboard layout.** Spec: none yet → specify first.

Found on 2026-09-23 while stripping comments for C10 (PR #70), by reading the code; not
yet tried on a non-QWERTY layout.

`isFarEndToggle` in `ui/src/adapters/keyboard.ts` recognises Ctrl+Shift+K by
`event.key === 'k' || event.key === 'K'` with `ctrlKey` and `shiftKey`. The comment above
it, deleted in C10, said the toggle is matched on the physical letter rather than on
`event.key`. The code does the opposite: `event.key` is the character the active layout
produces, and `event.code` (`KeyK`) is the physical key. On a layout where the key
labelled K produces another character, or where K sits elsewhere, the toggle follows the
character, not the key position the comment described.
