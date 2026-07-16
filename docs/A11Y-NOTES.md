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
