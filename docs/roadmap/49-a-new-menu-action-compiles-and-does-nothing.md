# 49 — A new menu action compiles and does nothing.

Lane: lane-1-ui. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**A new menu action compiles and does nothing.** Spec: none yet → specify first.

Found on 2026-09-23 while stripping comments for C10 (PR #70), by reading the code.

`MenuAction` is generated from the Rust enum into `ui/src/protocol.ts`, and the frontend
acts on it with `switch (action)` in `ui/src/adapters/system_menu.ts`. The comment above
that switch, deleted in C10, said the switch is exhaustive, so a variant the backend adds
"fails to compile here". Nothing enforces that. The switch has no `default` arm assigning
`action` to `never`, and `ui/tsconfig.json` (`strict`, `noUncheckedIndexedAccess`) has no
option that makes a missing case an error. A `MenuAction` variant added in Rust
regenerates the union, `tsc` passes, and choosing that menu item does nothing at all,
without a sound or an error.
