# Spec: PR 0 — workspace scaffold

Contract for the first PR. No logic lands here; the deliverable is a repository in
which every later PR has an obvious home and CI that enforces the rulebook.

## Deliverables

- Cargo workspace at the repo root with five member crates:
  - `crates/acter-core`
  - `crates/acter-term`
  - `crates/acter-transports`
  - `crates/acter-shells`
  - `crates/acter-app`
- Each crate: facade `lib.rs` containing only a crate-level `//!` doc comment (one
  sentence: the crate's role) and `#![warn(unreachable_pub)]`. No modules yet.
- Workspace-level `Cargo.toml` with shared lints table (clippy warnings denied) and
  shared package metadata (edition, license).
- `rustfmt.toml` (defaults; the file exists so the config is explicit).
- GitHub Actions workflow on a Windows runner: `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- `docs/A11Y-NOTES.md` created with the finding-entry template (NVDA version,
  date, expected, observed, spec/PR reference).

## Acceptance criteria

- Fresh clone + `cargo build --workspace` succeeds on Windows.
- CI runs and is green on the PR.
- No `pub` item exists anywhere (nothing to export yet), so `unreachable_pub`
  produces no warnings.

## Out of scope

Tauri initialization, the `ui/` project, any dependency beyond the toolchain
(serde etc. arrive with the code that needs them).

## Definition of done

Merged to main with green CI; ROADMAP.md's PR 0 entry checked off in the same PR.
