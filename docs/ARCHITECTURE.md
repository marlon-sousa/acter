# Acter — Technical Architecture

Companion to [DESIGN.md](DESIGN.md), which owns product/functional decisions. This
document owns code organization and engineering practice.

**Status: the whole document is Decided** (workspace layout, hexagonal ports/adapters,
DI via constructor injection at the composition root, sync-core/async-edges paradigm,
actor-style sessions, IPC types via tauri-specta, no-framework frontend, four-tier
test strategy, module conventions). New sections state their own status if different.

## Guiding principle

Hexagonal architecture (ports and adapters): `acter-core` defines traits (ports);
concrete I/O implementations (adapters) plug in at the edges; the Tauri app is the
composition root. Dependency arrows only point inward.

## Workspace layout

Cargo workspace:

- `crates/acter-core` — the domain. Zero Tauri, zero I/O dependencies. Owns:
  - session model, profile/config model
  - OSC 133 command-boundary state machine
  - auto-read policy
  - completion logic (phase 1: history + paths)
  - trait definitions (ports): `Transport`, `ShellAdapter`, `TerminalEngine`
  - IPC protocol types (serde) — see "IPC protocol" below
- `crates/acter-term` — wraps `alacritty_terminal` behind the `TerminalEngine` trait:
  bytes in; grid state, extracted text, alt-screen transitions out. Separate crate to
  keep the heavy dependency out of core and the engine swappable.
- `crates/acter-transports` — `LocalPty` (ConPTY via `portable-pty`) and `Ssh`
  (`russh`, behind a feature flag; phase 1 builds don't pay for it).
- `crates/acter-shells` — PowerShell / cmd / bash adapters: shell-integration
  injection snippets, quoting rules, completion strategy.
- `crates/acter-app` — Tauri 2 app and composition root: wires concrete adapters into
  core sessions, exposes IPC commands, bridges session events to Tauri events. As thin
  as possible.
- `ui/` — frontend (outside the Cargo workspace).

## Code organization within crates — **Decided**

Heavy module splitting; visibility and layout follow four mechanical conventions:

1. **`lib.rs` is a facade**: only `mod` declarations and `pub use` re-exports, zero
   logic. The crate's public API is readable in one screen; consumers write
   `acter_core::Session`, never deep paths.
2. **Visibility ladder**: private by default → `pub(crate)` when another module in
   the same crate needs it → bare `pub` only on items re-exported from `lib.rs`.
   Enforced with `#![warn(unreachable_pub)]` in every crate.
3. **File convention**: modern style — entry point `<module>.rs` with internals in
   `<module>/` (never `mod.rs`; distinct filenames also read better in editor tabs
   with a screen reader). One concept per file, named after what it holds. A module
   earns a folder when it has genuinely private internals or crosses a few hundred
   lines — split by concept, not by size reflex.
4. **Tests live with the code**: `#[cfg(test)] mod tests` at the bottom of the file
   under test (sees private items — no test-only exports); integration tests and
   golden transcript fixtures in each crate's `tests/`.

### Module role rule — **Decided**

Every module plays exactly one of six roles, declared on the first line of its `//!`
doc comment:

1. **Entity/value** — domain data plus its invariants (session/state.rs,
   profile/settings.rs, protocol types).
2. **Policy** — pure domain computation, deterministic in → deterministic out
   (autoread.rs, boundary/, completion/path.rs, profile inheritance).
3. **Port** — a trait seam, in either direction. **Driven ports** define what the
   domain needs from the world (ports/driven/: Transport, ShellAdapter,
   TerminalEngine, Clock, Notifier) — adapters implement them. **Driving ports**
   define what the world may ask of the domain (ports/driving/: SessionApi,
   CompletionApi) — services implement them, controllers depend on them.
4. **Adapter** — a port implementation that touches the world (acter-transports,
   acter-shells, acter-term).
5. **Service** — a domain's actionable surface: named use cases in domain vocabulary
   (SessionService::submit_command / toggle_mode, CompletionService::complete).
   Coordinates entities, policies, and ports for one domain action; touches the world
   only through injected driven ports, so it is deterministic under fake ports and
   testable without any runtime. **Every service consumed by a controller is fronted
   by a driving-port trait** — controllers hold `Arc<dyn SessionApi>`, never the
   concrete type; only the composition root names `SessionService`. Naming
   convention: trait `XxxApi` (driving port), implementation `XxxService`. Service
   trait methods stay synchronous (consistent with sync-core/async-edges; also keeps
   traits dyn-compatible without async_trait machinery).
6. **Controller (orchestrator)** — the delivery layer: translates outside-world
   events into service calls and results into outside-world effects; owns runtime
   machinery (async tasks, channels, Tauri IPC). The session actor loop and the Tauri
   command handlers are controllers. Litmus test: deleting a controller loses no
   business behavior, only connectivity.

Classifying question: **does the module do anything nondeterministic — I/O, time,
randomness, environment?** Yes → adapter, and it deserves a port. No → one of the
other five. Still doesn't fit → the module is misfactored; split it until each piece
fits. ("Doesn't fit" is a smell to refactor away, not a bucket.)

Layering is a one-way street: controllers → driving ports (implemented by services)
→ entities/policies/driven ports (implemented by adapters). Nothing reaches around a
service to poke a domain's internals from outside it, and no controller names a
concrete service type.

Testing matrix this yields: controllers tested against fake services (IPC ↔ use-case
translation), services against fake driven ports (domain behavior), policies pure
(no fakes at all). Every layer isolated, every seam substitutable.

Consequences and guardrails:
- No junk-drawer modules (utils.rs, helpers.rs are banned by construction).
- Ports exist to isolate nondeterminism, so pure logic never gets a trait wrapper
  just for ceremony — a pure parser is already testable.
- **Service sprawl guard:** a service must coordinate at least two of
  {port, entity, policy} for a named use case. A single pass-through method does not
  get a service — the controller calls the target directly.
- Known upcoming ports this rule forces: `Clock` (anything time-based) and
  `Notifier` (beep/audio) — tempting to inline, nondeterministic, therefore ported.

### Reference layout

acter-core/src:
- lib.rs (facade)
- ports.rs + ports/: driven/ (transport.rs, shell_adapter.rs, terminal_engine.rs,
  clock.rs, notifier.rs) and driving/ (session_api.rs, completion_api.rs) — one
  folder holds every seam in the system
- session.rs + session/: state.rs (mode/lifecycle state machine — entity),
  service.rs (SessionService — the session domain's use cases), manager.rs
  (session collection that tabs map onto — controller)
- boundary.rs + boundary/: osc133.rs (sequence recognition), tracker.rs
  (command-block state machine)
- autoread.rs (threshold policy — one concept, no folder needed)
- profile.rs + profile/: settings.rs, inheritance.rs (Defaults resolution)
- completion.rs + completion/: history.rs, path.rs
- protocol.rs + protocol/: events.rs, commands.rs (specta-annotated IPC types)

acter-transports/src:
- lib.rs (facade)
- local.rs + local/: conpty.rs, reader.rs (blocking-read thread)
- ssh.rs + ssh/ (feature "ssh"): connection.rs, auth.rs

acter-shells/src stays flat until modules earn folders: powershell.rs, cmd.rs,
bash.rs, integration.rs (shared OSC 133 snippet templates).

## Dependency injection

No DI framework. Constructor injection at the composition root: core types take
collaborators as trait objects (`Box<dyn Transport>` etc.); only `acter-app` names
concrete types. Tests inject hand-written fakes (e.g. `FakeTransport` replaying a
scripted byte stream — the interesting behavior IS the byte stream). `mockall` only
if a port ever needs interaction-verification; fakes are the default.

## Coding paradigm

- **Sync pure core, async edges.** Boundary parser, auto-read policy, session state:
  synchronous state machines (enums + pattern matching). Async (tokio — Tauri brings
  it) only in the orchestration layer.
- **Actor-style sessions:** each session is one tokio task owning its transport and
  terminal engine, communicating via channels. No shared mutable state; a hung SSH
  connection cannot stall another tab. Blocking PTY reads get dedicated threads
  feeding the session channel.
- **Errors:** `thiserror` in library crates; `anyhow` only in `acter-app`. Every
  user-facing error must have a speakable message — domain requirement, not polish.
- **Logging:** `tracing`.

## IPC protocol

Event/command types (output chunk, command boundary + exit code, alt-screen
entered/left, resize, completion request/response, mode toggle, connection state) are
serde types in `acter-core`. TypeScript definitions are generated from them with
`tauri-specta`, so the protocol lives in exactly one place and the frontend cannot
silently drift.

## Frontend

Plain TypeScript + Vite, no framework. The frontend is ARIA attributes, focus
management, and live regions — direct DOM control is a feature (a virtual-DOM
re-render that recreates a live-region node kills pending announcements). State is
small: session list, buffer blocks, mode flag.

## Test strategy

Four tiers, cheapest first:

1. **Pure unit tests** (core): byte sequences through the boundary state machine;
   table tests for the auto-read policy. **Property tests** (`proptest`): the OSC
   parser never panics on arbitrary bytes.
2. **Golden transcript tests** (the workhorse): raw byte captures from real
   PowerShell/cmd/bash sessions committed as fixtures, replayed through the
   term + boundary pipeline, asserting extracted command/output blocks. Catches
   real-world escape-sequence weirdness without spawning shells on every run.
3. **Integration tests**: spawn a real ConPTY, run trivial commands end-to-end.
   Windows CI runner, separate job.
4. **Accessibility**: automated axe-core checks on the built DOM in CI (missing
   roles, detached live regions); manual NVDA pass against a written checklist per
   release — automation cannot hear speech, and here the speech is the product.

## Tooling floor

- `clippy` with warnings denied, `rustfmt` enforced.
- CI on a Windows runner from day one.
