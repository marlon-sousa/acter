# 30 — Whether a widget's selection is visible to a text diff.

Lane: keyboard-routing. Board: [../ROADMAP.md](../ROADMAP.md). The entry is closed with no spec of its own; this file keeps its evidence.

**Closed 2026-09-02 — measured, and the answer went into 28.** This entry existed to
find out whether a widget's selection is visible to a text diff at all, because a
highlight drawn in colour alone would have needed attribute-aware diffing and a decision
about how a "selected" row is expressed to a screen reader. Three prompt-driven samples
answered it and none of them reopened anything, so what is left is the measurement note
rather than an entry to build.

- **`gh repo create`, 2026-08-31, gh 2.96.0**, aborted at the first prompt. No alternate
  screen. The selection is a `>` at the start of the row, with colour used as well as the
  marker rather than instead of it. Each arrow rewrites exactly two rows, and the engine
  reports exactly those two, because it emits only lines whose text changed. Both arrow
  encodings accepted; the selection wraps at both ends.
- **`gh pr create`, 2026-09-02, same version**, aborted before anything was pushed. The
  same shape, and two findings nobody sought: the cursor is hidden for the whole prompt
  and parked below the list, so this is a rewritten-row rule and never a cursor-row one;
  and answering the prompt erases it — the question row became `? Where should we push
  the '…' branch? Cancel` and the three option rows became empty — which is what settles
  that the buffer applies revisions by id, blanks included, and keeps nothing else.
- **PSReadLine's completion menu, 2026-09-02**, `pwsh` 7.6.5 with Tab bound to
  `MenuComplete`. **The selection is drawn in colour alone**, with no marker character
  anywhere, which is what a rule naming `>` would have been unable to hear — and it is
  why the rule is the row that gained non-whitespace content. The menu is also not where
  the answer is: each arrow rewrites the command line itself, one completion per press,
  so the anchored row already carries it. And the repaint is large — eleven line items on
  the first press — which is what settled that row count routes nothing.

All three are written into DESIGN under "A row that changed is an answer", and the rule
they produced shipped in 28 as `policies::far_end_row`. Nothing is owed here.
