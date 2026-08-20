# CLAUDE.md — agent contract for Acter

Acter is an accessibility-first terminal for screen reader users. Rust + Tauri 2
(HTML frontend over WebView2), Windows first. Development is AI-first: the repository
is the source of truth, and specs are written before code.

## Read before designing or coding

- [docs/DESIGN.md](docs/DESIGN.md) — product/functional decisions and open questions.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — engineering rules: crates, module
  role rule, ports/services, DI, test strategy.
- [docs/ROADMAP.md](docs/ROADMAP.md) — PR-by-PR build order **and status board**:
  the answer to "what should we do now". Each entry says whether the next step is
  writing a spec or implementing one; the implementing PR flips its own entry to
  Done so the board is correct on main the moment the user merges.
- [docs/specs/](docs/specs/) — one spec per roadmap PR, agreed in conversation
  before implementation and committed in the PR it covers. The spec is the
  implementation contract: acceptance criteria, files touched, definition of done.

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
- PRs are short: one component + its trait(s) + unit tests. Nothing lands untested.
- Every user-facing string must be speakable by a screen reader — error messages are
  a domain requirement, not polish.
- All documentation and communication must be screen-reader friendly: no ASCII-art
  diagrams, no box-drawing trees. Prose, lists, and headings only.
- Manual accessibility checklists and their results go in the implementing PR's
  body as checkboxes — one item per check, findings written inline on the unchecked
  item (NVDA version, expected vs observed). There is no separate findings document.
  Findings that require changes become iteration entries in ROADMAP.md.
- **Who may run a checklist item.** An agent may run and record any item it can
  actually observe, driving a real screen reader through the screen-readers MCP bridge
  and recording what was spoken. Such an item is checked by the agent, and its
  observation is written inline exactly as a human's would be, naming the reader
  version and the capture mode. Items an agent **cannot** observe stay the human's to
  verify and check — the bridge captures speech and braille, not audio, so anything
  turning on a beep, a sound cue, or subjective comfort is human-only. State plainly in
  the PR body which items were agent-observed and which were human-verified; a checked
  box must never imply a sense nobody used. Mode surprises are the standard trap here:
  the tester's NVDA does not switch focus mode automatically, so an agent must read the
  current mode and set the one it needs rather than assume, and must not report a mode
  artifact as a defect in the software under test.
- **Connect to the bridge as the `user` persona, not `validator`.** Acter is for screen
  reader users, so what a checklist item asks is whether an ordinary, non-expert user can
  hear and do the thing — and that question is only answered by driving the way one drives:
  focus, tab, the arrows, typing, and the reader's ordinary reading commands. A stance with
  more reach answers a different question, and an item checked from it would claim
  reachability the product has not earned. `validator` is for one purpose, and it is stated
  when used: characterising a UI failure already found, where introspection says *what* is
  wrong rather than only that something is. Never to get past a failure. `expert` is for
  working out how the reader itself behaves, not for judging Acter.
