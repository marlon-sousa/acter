# Accessibility findings — manual NVDA sessions

Record every manual screen reader finding here, one entry per finding, newest first.
Iteration PRs (A5 and later in the roadmap) reference entries from this file.

## Entry template

Copy this block for each finding:

### YYYY-MM-DD — short title

- NVDA version:
- Windows version:
- Context: spec or PR being tested
- Expected:
- Observed:
- Follow-up: none, or the roadmap entry / issue that tracks the fix

## Findings

### 2026-07-16 — F6 into the results buffer: no browse mode, position not on last command

- NVDA version: (fill in)
- Windows version: 11 Pro
- Context: PR #2, a1-static-shell spec, manual checklist item 5
- Expected: F6 into the buffer leaves the user reading the most recent command
  block, with browse-mode quick navigation immediately usable.
- Observed: focus moves to the region container; NVDA does not switch to browse
  mode; the reading position is not on the last heading.
- Analysis: web content cannot force NVDA's mode — NVDA decides, and its automatic
  mode switching reacts to *what kind of element* receives focus. A bare focused
  `div role="region"` is a generic container: it does not reliably trigger NVDA's
  mode re-evaluation or virtual-caret sync (WebView2 exposes it as a grouping). The
  landing position is simply as coded: `BufferDom.focus()` focuses the container,
  not a block.
- Follow-up: first A5 iteration — give each command h2 `tabindex="-1"` and make
  F6-to-buffer focus the **most recent heading** (container as empty-buffer
  fallback). A focused heading is real content: NVDA syncs the virtual caret to it,
  announces it with its level, and (with default "automatic focus mode for focus
  changes") leaves focus mode since a heading does not require it.

### 2026-07-16 — A1 implementation choices awaiting NVDA validation

- Context: docs/specs/a1-static-shell.md (implementation, pre-testing)
- Edit field label: chose a **visible label** ("Command input") over aria-label, so
  sighted collaborators and screen reader users read the same thing.
- Results buffer: `role="region"` with `aria-label="Results"` and `tabindex="-1"`
  for F6 focus; plain content (h2 per command, response text below), not a live
  region.
- Announcer strategy: single polite live region; each announcement replaces the
  region's children with a fresh node, so repeating an identical response still
  mutates the DOM and should re-announce. To be validated in the first NVDA session
  (spec checklist items 2 and 5).
- Follow-up: first manual NVDA session against the spec checklist.
