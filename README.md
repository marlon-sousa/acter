# Acter

An accessible terminal for screen reader users.

Acter's default mode is conversational: you type a command in an edit field, and the
result appears in a reviewable buffer where each command is a heading — so screen
reader users navigate their session history the way they navigate a web page. Output
small enough to listen to is read automatically; anything bigger is announced and
signaled with a beep. A keystroke switches to full terminal emulation for programs
like nano or any ncurses app.

Built in Rust with Tauri 2 (HTML frontend over WebView2), Windows first.

## Status

Planning complete, implementation starting. Development is AI-first and spec-driven.

## Documentation

- [Design](docs/DESIGN.md) — product decisions and open questions.
- [Architecture](docs/ARCHITECTURE.md) — engineering rules.
- [Roadmap](docs/ROADMAP.md) — build order and PR plan.
- [CLAUDE.md](CLAUDE.md) — the agent contract for AI-first development.

## Development

### Machine setup (once per machine)

1. Git (plus the GitHub CLI for the PR workflow).
2. Visual Studio Build Tools with the "Desktop development with C++" workload —
   required by both the Rust MSVC toolchain and Tauri's Windows build.
3. rustup, stable toolchain (defaults to MSVC on Windows) — brings cargo.
4. Node.js 22+ — brings npm. All JS tooling (Vite, vitest, TypeScript, Tauri CLI)
   is installed per-project and version-locked; nothing global.
5. WebView2 runtime — already present on current Windows 10/11.
6. NVDA, for manual accessibility checklists.

### Project setup (once per clone)

- `npm install` inside `ui/` (installs Vite, vitest, and the pinned Tauri CLI).

The dev loop (from `ui/`): `npm run tauri dev` — starts the Vite dev server,
compiles the Rust side, and launches the app with hot reload on both halves.
`npm run tauri build` produces the release build. The Tauri CLI is pinned in
`ui/package.json`; installing `cargo install tauri-cli` and using `cargo tauri dev`
from the repo root works identically if you prefer cargo as the entry point.

Rust checks (from the repo root):

- Build everything: `cargo build --workspace`
- Run all tests: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format check: `cargo fmt --all --check`

Frontend checks (from `ui/`):

- Typecheck: `npm run typecheck`
- Tests: `npm test`

## License

See [LICENSE](LICENSE).
