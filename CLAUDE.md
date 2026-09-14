# CLAUDE.md — agent contract for Acter

Acter is an accessibility-first terminal for screen reader users. Rust + Tauri 2
(HTML frontend over WebView2), Windows first. Development is AI-first: the repository
is the source of truth, and specs are written before code.

## What to read, by kind of session

- Every session that writes code reads [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
  (engineering rules: crates, module role rule, ports/services, DI, test strategy) and
  the spec for its PR under [docs/specs/](docs/specs/). The spec is the implementation
  contract: acceptance criteria, files touched, definition of done. Nothing else is
  required reading for coding.
- A session that changes what the user hears or does also reads
  [docs/DESIGN.md](docs/DESIGN.md), the product decisions and open questions. Other
  sessions open the section they need.
- A planning session reads [docs/ROADMAP.md](docs/ROADMAP.md), the status board, and
  starts from its "What is next" section. The board is one line per entry; an open
  entry's finding is one file under [docs/roadmap/](docs/roadmap/), opened for the one
  entry being decided and not before. Recording a finding, promoting an entry to a spec
  and flipping a line to Done are the `roadmap` skill; the implementing PR flips its own
  line, so the board is correct on main the moment the user merges.

## Process

- Specs are approved before coding, but they travel with the code: the spec file
  lands in the PR it belongs to (or the first PR it covers), not on main ahead of
  it. Agree the spec in conversation first; then open one PR that contains both the
  spec and its implementation, judged against that spec. If implementation forces a
  spec amendment, the amendment rides in the same PR.
- Design/architecture decisions (DESIGN.md, ARCHITECTURE.md, ROADMAP.md) are not
  specs: they are approved in conversation and land directly on main.

## Hard rules

- Items marked **Decided** in the docs are settled. Do not relitigate them silently;
  to change one, propose it explicitly and update the doc in the same PR that
  implements the change.
- Module role rule: every module is exactly one of entity/value, policy, port,
  adapter, service, controller — declared on the first line of its `//!` doc comment.
  Full definitions in ARCHITECTURE.md.
- Visibility ladder (private → `pub(crate)` → re-exported `pub`), facade `lib.rs`,
  `module.rs` + `module/` folders (never `mod.rs`), no junk-drawer modules.
- **Comments say what the code cannot.** A comment is the module role line, a measured
  fact and what it was measured against, an invariant or hazard the code does not enforce, or what an absent
  value or an error carries. One sentence, at the point of use, once. Rationale, history,
  alternatives and spec or decision citations go in the spec and the PR body, never in
  code. No doc paragraph on a field, method or test whose name already says it. Every PR
  body carries the total line of `python scripts/comment_ratio.py` for the files touched.
  Rubric: [docs/specs/c-comments-say-what-the-code-cannot.md](docs/specs/c-comments-say-what-the-code-cannot.md).
- PRs are short: one component + its trait(s) + unit tests. Nothing lands untested.
- Every user-facing string must be speakable by a screen reader — error messages are
  a domain requirement, not polish.
- All documentation and communication must be screen-reader friendly: no ASCII-art
  diagrams, no box-drawing trees. Prose, lists, and headings only.
- Manual accessibility checklists and their results go in the implementing PR's
  body as checkboxes — one item per check, findings written inline on the unchecked
  item (NVDA version, expected vs observed). There is no separate findings document.
  Findings that require changes become iteration entries in ROADMAP.md.
- Running a checklist item, whether an agent may check it, and which bridge persona to
  drive with, is a procedure: invoke the `accessibility-checklist` skill before touching
  the screen-readers bridge.
