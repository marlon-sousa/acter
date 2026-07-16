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

## License

See [LICENSE](LICENSE).
