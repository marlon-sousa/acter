# 39 — Four smaller things the same run measured, each its own fix.

Lane: lane-3-macos. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**Four smaller things the same run measured, each its own fix.** Spec: none yet →
specify first. All 2026-09-03, VoiceOver 15.0, silent capture, `user` persona.

- **The Connect button is not in the tab order until a shell is chosen.** With the
  Terminal row selected and no shell picked, `Tab` goes from the set-up checkbox to the
  help button to **Cancel**, and nothing says why. Arrowing the shell list once puts
  Connect back. A control that is absent rather than disabled-and-explained is the failure
  B5.4, decision 2 wrote `(not available)` into a label to avoid.
- **F6 does not toggle back.** It moves focus to the Results region and, pressed again
  there, leaves it there; `Escape` is what returns to the command line. A9 calls F6 a
  toggle.
- **Every connect-list row is announced twice** — `SSH`, `SSH, selecionado (2 de 6)`,
  then both again.
- **The set-up dialog announced itself in one run and not in another.** In the first,
  keyboard focus stayed on the web-content root, nothing was spoken, and the dialog was
  only findable by reading the whole window; in the second it read itself immediately.
  Inconsistent between runs, so the first step is a reproduction rather than a fix.
