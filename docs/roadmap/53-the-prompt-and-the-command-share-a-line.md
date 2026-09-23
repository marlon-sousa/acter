# 53 — V2, the prompt, the command and the edit field share one visual line.

Lane: lane-5-look. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**V2, the prompt, the command and the edit field share one visual line.** Spec: none yet → specify first.

Found on 2026-09-23 in the conversation that agreed V1, by reading `ui/src/adapters/buffer.ts`.
The buffer holds a prompt as a `p`, then the command as an `h2`, then its output rows, as
siblings in that order, and the edit field is a separate form after the buffer. A real
terminal draws the prompt and the command on one line and the cursor after the last prompt,
so on screen Acter shows the prompt and the command on separate lines.

Two things are unmeasured. NVDA's browse mode decides what counts as one line partly from
the layout (its "use screen layout" setting), so two elements drawn on one visual line may
be read as one line. And in far-end line mode it is not known whether the far end's row
already includes the prompt text, which would show it twice.
