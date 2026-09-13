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

## Installing

Two ways, and which one you choose decides where Acter keeps its settings.

**The installer** — `acter_x.y.z_x64-setup.exe` from the
[releases page](https://github.com/marlon-sousa/acter/releases). It installs for your
account only and never asks for administrator rights. Its settings live with the rest of
your application data, in `%APPDATA%\acter\settings`.

**The portable zip** — `acter-windows-vx.y.z-portable.zip`. Unpack it anywhere, including
a USB stick, and run `acter.exe`. It keeps its settings in a `settings` folder beside the
program, so the whole of Acter travels with the folder you put it in.

Either way, Help then About Acter reads out where that folder is on this computer, and how
Acter came to be using it. Setting `ACTER_SETTINGS_DIR` to a folder of your own overrides
both.

**Windows will warn you before it runs either of them.** Neither is signed with a code
signing certificate yet, so SmartScreen says the publisher is unknown; choose "More info"
and then "Run anyway". This is a known cost, recorded rather than worked around: a
certificate is a purchase, and until there is one, saying so plainly is more use than
pretending the warning is not there.

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

- `npm install` at the repo root (npm workspaces install the `ui/` dependencies,
  including the pinned Tauri CLI).

Everything below runs from the repo root.

The dev loop: `npm run dev` — starts the Vite dev server, compiles the Rust side,
and launches the app with hot reload on both halves. `npm run build` produces the
release build. Other Tauri CLI commands: `npm run tauri -- <args>`.

Rust checks:

- Build everything: `cargo build --workspace`
- Run all tests: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format check: `cargo fmt --all --check`

Frontend checks:

- Typecheck: `npm run typecheck`
- Tests: `npm run test:ui`

### Troubleshooting

- **404 page inside the app window:** an orphaned Vite process is squatting port
  5173 (Windows sometimes leaks the dev-server child when the app closes). Find it
  with `Get-NetTCPConnection -LocalPort 5173 -State Listen`, stop the owning
  process, and run `npm run dev` again.

## License

See [LICENSE](LICENSE).
