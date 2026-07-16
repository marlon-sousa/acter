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
- `ui/` — the frontend: an npm workspace member, not a crate, deliberately outside
  the Cargo workspace. The repo has **two build worlds** — the Cargo workspace
  (crates/) and the npm workspace (ui/, later e2e/) — connected at exactly one
  seam: acter-app consumes ui's build, declared in tauri.conf.json's build section
  (devUrl proxies the Vite dev server in dev; frontendDist is embedded into the
  executable in release). Runtime coupling is the IPC protocol; build coupling is
  that one stanza.

## Code organization within crates — **Decided**

Heavy module splitting; visibility and layout follow six mechanical conventions:

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
5. **Folders are organized by module role, one file per concept.** Within a crate
   (and within ui/src), the top-level folders are the roles: `entities/`,
   `policies/`, `ports/` (driven/driving), `services/`, `controllers/`, `routers/`,
   `adapters/`, plus the container file (`container.rs` / `main.ts`). One file per
   port, service, adapter, controller, router. The tree is the architecture
   diagram: the role is visible in the path before opening the file; the filename
   carries the concept. Concept folders (session/, profile/) do not exist — a
   concept spanning several roles is several files in several role folders. Adapter
   crates (acter-transports, acter-shells, acter-term) are role folders promoted to
   crate rank; an adapter with private internals earns a subfolder as usual.
6. **Import the symbol, then use it by bare name.** Do not reach a symbol through an
   inline module path. Rust: `use serde_json::json;` then `json!(...)`, never
   `serde_json::json!(...)`; `use tauri::Builder;` then `Builder::default()`, never
   `tauri::Builder::default()`. This covers types, functions, values, and macros —
   including attribute macros (`use tauri::command;` then `#[command]`, not
   `#[tauri::command]`). JS/TS: named imports (`import { invoke }`), never namespace
   imports (`import * as api`) or inline qualified access. **`::` is not always a
   module path**, and the following stay as-is — they already are the
   import-then-use pattern, not a violation:
   - An associated function or enum variant on a type that is itself imported:
     `Arc::new(..)`, `Builder::default()`, `InvokeBody::Json(..)`. Import the *type*,
     then call through it; do not try to import the associated item.
   - Trait-method disambiguation through a prelude trait: `Default::default()`.
   - `crate::` / `super::` / `self::` module-locating paths that a macro needs to
     resolve generated companion items — e.g. the command path in
     `generate_handler![crate::routers::echo]`, which cannot be a bare import because
     the macro also references the hidden `__cmd__echo` sibling.

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
   acter-shells, acter-term). The framework counts as the world: the Tauri **router**
   layer (`#[tauri::command]` one-liners delegating to controllers through traits)
   and the Tauri `EventSink` emitter are adapters living in acter-app.
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
6. **Controller (orchestrator)** — the delivery layer: translates protocol payloads
   into service calls and results into protocol events; owns runtime machinery
   (async tasks, channels). Controllers are **framework-free** plain Rust: they hold
   `Arc<dyn SessionApi>` and `Arc<dyn EventSink>`, are reached from routers through
   traits, and are tested with fakes under plain cargo test — no Tauri runtime. The
   session actor loop and the per-domain command controllers are controllers. Litmus
   test: deleting a controller loses no business behavior, only connectivity.

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
- Known upcoming ports this rule forces: `Clock` (anything time-based), `Notifier`
  (beep/audio), and `EventSink` (event emission to the frontend) — tempting to
  inline, world-touching, therefore ported.

### Reference layout

acter-core/src (the domain crate — the crate is the domain, so no domain/ wrapper):
- lib.rs (facade)
- entities.rs + entities/: session_state.rs (mode/lifecycle state machine),
  profile.rs, protocol_events.rs, protocol_commands.rs (specta-annotated IPC types)
- policies.rs + policies/: autoread.rs (threshold/pacing policy), osc133.rs
  (sequence recognition), boundary_tracker.rs (command-block state machine),
  profile_inheritance.rs (Defaults resolution), completion_history.rs,
  completion_path.rs
- ports.rs + ports/: driven/ (transport.rs, shell_adapter.rs, terminal_engine.rs,
  clock.rs, notifier.rs, event_sink.rs) and driving/ (session_api.rs,
  completion_api.rs) — one folder holds every seam in the system
- services.rs + services/: session.rs (SessionService), completion.rs
- controllers.rs + controllers/: session_actor.rs (per-session loop),
  session_manager.rs (session collection that tabs map onto)

acter-app/src (delivery + container):
- lib.rs (facade), main.rs (entry point)
- container.rs — the composition root
- routers.rs + routers/: one file per router (echo.rs). The routers.rs facade uses
  glob re-exports (`pub(crate) use echo::*;`) because `#[tauri::command]` generates
  hidden companion items that `generate_handler!` needs alongside the function.
- ports.rs + ports/ and services.rs + services/: the temporary echo harness
  (echo_api.rs, echo.rs) until A3 wires the real core domain
- adapters/ (later): the Tauri EventSink emitter

acter-transports/src (adapter crate):
- lib.rs (facade)
- local.rs + local/: conpty.rs, reader.rs (blocking-read thread)
- ssh.rs + ssh/ (feature "ssh"): connection.rs, auth.rs

acter-shells/src (adapter crate) stays flat until modules earn folders:
powershell.rs, cmd.rs, bash.rs, integration.rs (shared OSC 133 snippet templates).

ui/ (mirror hexagon, same convention). Everything shipped lives under src/;
tests live under a sibling test/ tree (see "UI source and test layout" below):
- src/views/: main_window.html — declarative HTML view, one per window
- src/main.ts — the container (composition root)
- src/ports/: one interface per file — backend_api.ts, edit_field_view.ts,
  buffer_view.ts, announcer_view.ts
- src/routers/: tauri.ts (the only module importing @tauri-apps/api)
- src/controllers/: app.ts
- src/adapters/: edit_field.ts, buffer.ts, announcer.ts, keyboard.ts
- test/: mirrors src/ — test/controllers/app.test.ts, test/adapters/buffer.test.ts

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

## IPC protocol and communication flow — **Decided**

Event/command types (output chunk, command boundary + exit code, alt-screen
entered/left, resize, completion request/response, mode toggle, connection state) are
serde types in `acter-core`. TypeScript definitions are generated from them with
`tauri-specta`, so the protocol lives in exactly one place and the frontend cannot
silently drift.

### The two directions

- **JS → Rust: Tauri commands (invoke), through routers.** `#[tauri::command]`
  functions are **routers**: framework adapters of one line each, delegating to a
  controller through a trait taken from managed state. Controllers are framework-free
  (see module role rule); Tauri-specific signatures stop at the router. Routers, as
  pure glue with no branches, share the facade-lib.rs exemption from the testing
  rule — the specta-generated types compile-check their contract.
- **Rust → JS: Tauri IPC Channels, through the `EventSink` driven port.** Controllers
  and session actors emit protocol events via `EventSink` (`send(SessionEvent)`);
  tests inject a recording fake. The production adapter wraps a
  `tauri::ipc::Channel` established at `attach_session`: the frontend creates a JS
  `Channel<SessionEvent>` and passes it in the invoke; the per-session envelope
  stream flows down it. Channels, not broadcast events, because session output is a
  sustained stream (`tail -f` never ends): Channel gives ordered, caller-bound
  delivery on a fast path, while Tauri events broadcast to all listeners and are not
  meant for high-throughput streaming. Broadcast events remain only for rare
  app-level notifications (session list changed, config updated). Attachment is
  handled by the session actor, so channel registration and the snapshot reply are
  serialized — no event can slip between snapshot and channel going live. No domain
  or delivery code ever holds an `AppHandle` or a Channel directly.

Consequence: the entire application — controllers, services, fake transports, full
round trips including emitted events — runs under plain cargo test with no Tauri
runtime. The convergence PR carries an integration test that submits a command and
asserts the exact event sequence the frontend would receive.

### Rules

- **Invoke is for actions and queries; events are for streams. An invoke never waits
  on the shell.** `submit_command` returns immediately with an ack and a
  **correlation id** (`command_id`); all subsequent events about that command carry
  the same id. Completions, mode toggles, profile CRUD, snapshots: invokes. Output,
  boundaries, alt-screen, connection state: events.
- **One event channel, one envelope.** A single `session-event` whose payload is the
  protocol enum (serde-tagged). specta generates a TS discriminated union, so the
  frontend compiler forces exhaustive handling of every variant.
- **Policy decisions ride the events.** Every announcement-bearing event — quiescent
  output chunks and `CommandFinished { command_id, exit_code, read_mode }` alike —
  carries the read verdict (`Auto` | `TooBig` | quiet) computed backend-side by the
  pacing policy (see DESIGN.md, Output pacing); the frontend obeys, never
  re-measures. Policy stays in core.
- **Output coalescing.** The session actor batches PTY output and flushes on a short
  tick (tens of ms) or on a boundary event, whichever comes first — the IPC bridge
  and DOM never see per-write traffic.
- **Snapshot recovery.** The session actor is the source of truth; the frontend holds
  view state only. `get_session_snapshot` rebuilds the whole UI (a webview reload
  never kills sessions).

### Round trip example (submit "git status")

1. Frontend invoke `submit_command` → controller → `SessionApi::submit_command` →
   service writes to the transport port → returns `command_id`. UI appends the h2
   block immediately, tagged with the id.
2. PTY reader thread feeds the session actor; bytes run through `TerminalEngine` and
   the boundary tracker.
3. Actor emits `CommandStarted`, then coalesced `Output` events; UI appends under the
   matching block.
4. OSC 133 end marker → service applies auto-read policy → `CommandFinished` with
   `read_mode`; UI feeds the live region or announces too-big + beep.

### Frontend organization (mirror hexagon) — **Decided**

The six-role module rule applies to the TypeScript side too (role declared in each
file's header comment). Channels are one-way (Rust → JS; JS → Rust is always
invoke), so the inbound router lives in the frontend at `channel.onmessage`.

- **Views** (`ui/src/views/` — declarative HTML, one file per window; role declared
  in a header comment like every TS file). Views are source, so they live under
  `src/` with the rest of the shipped frontend, not in a sibling folder. The static
  semantic skeleton (roles, labels, live regions) lives in HTML so it is inspectable
  without executing anything — main_window.html is the entry (Vite
  `rollupOptions.input` + the Tauri window url `src/views/main_window.html`; a
  deliberate deviation from Vite's index.html default). Future windows land beside
  it. Dynamic DOM behavior never lives here — adapters only; repeated dynamic markup
  may use `<template>` elements inside the view, stamped by adapters.
- **Router** (`routers/` — the only module importing `@tauri-apps/api`). Inbound: owns
  `channel.onmessage`, exhaustive switch over the generated discriminated union,
  each variant translated into a controller call. Outbound: implements the
  `BackendApi` interface as typed invoke wrappers. An adapter of one-liners;
  exhaustiveness is compiler-checked against the generated union — same testing
  exemption as the backend router, same justification.
- **Controllers** (framework-free TypeScript): receive protocol events, hold view
  state, decide what changes; initiate actions only through `BackendApi`. Tested in
  vitest with a fake `BackendApi` and fake views — no Tauri, no DOM.
- **View adapters** (the DOM is the world): buffer view, live-region announcer,
  focus manager, beep player — each behind a small interface. The riskiest a11y
  behaviors (announcement timing, live-region node lifecycle) live here, thin and
  isolated, so a manual NVDA finding maps to one small file.

Both ends of the wire are fed by protocol types generated from the single Rust
source; the system reads identically in both languages: router (framework adapter)
→ controller (framework-free) → ports → adapters (world).

#### UI source and test layout — **Decided**

For npm packages (currently `ui`; the Rust crates keep unit tests inline in their
own files, so this does not apply to them):

- **Everything shipped lives under `src/`** — TypeScript and views alike. Views are
  source (declarative HTML consumed by the Vite build), so they live at
  `src/views/`, not in a sibling `views/` folder. `src/` is the single tree the
  build and the Tauri window url point into.
- **Tests live under a top-level `test/` tree that mirrors `src/`.** A test for
  `src/adapters/buffer.ts` is `test/adapters/buffer.test.ts`; it imports across the
  boundary (`../../src/adapters/buffer`). This keeps `src/` to shippable code (tests
  never crowd the source tree and sit nowhere the bundle can reach), while still
  colocating each test with the role folder of the code it exercises. `tsconfig.json`
  includes both `src` and `test`; vitest discovers `test/**/*.test.ts` by its
  default glob.

### Frontend DI — **Decided: no framework**

Same verdict as the Rust side. The object graph is small (roughly fifteen objects at
maturity) and wiring is linear; a hand-written **composition root in `main.ts`**
covers it and is fully compile-time checked — a missing dependency fails the build,
whereas containers resolve at runtime (and the decorator family — InversifyJS,
tsyringe — additionally requires annotating the classes themselves; the
non-decorator family does not, and binds in a central container file).
**Factory functions** handle the one dynamic scope: sessions/tabs —
`createSessionGraph(deps)` builds the per-session cluster, closing over app-wide
singletons.

**Construction rule (both languages):** constructing a collaborator is a privilege
of the composition root and its factories, nowhere else. No controller, service, or
adapter ever news up a dependency at the point of use; everything is received via
constructor. The composition root is our container.ts — same central binding list a
DI container would hold, expressed as typed constructor calls. This invariant, not
the tool, is what keeps layers decoupled, and it holds even if the escape hatch
below is ever exercised.

Escape hatch (recorded to avoid relitigating): if the composition root genuinely
hurts (hundreds of lines, conditional wiring), acceptable candidates are the
non-decorator containers only (awilix, typed-inject) — never the reflect-metadata
family.

## Frontend

Plain TypeScript + Vite, no framework. The frontend is ARIA attributes, focus
management, and live regions — direct DOM control is a feature (a virtual-DOM
re-render that recreates a live-region node kills pending announcements). State is
small: session list, buffer blocks, mode flag.

## Test strategy

Six tiers, cheapest first:

1. **Pure unit tests** (core): byte sequences through the boundary state machine;
   table tests for the auto-read policy. **Property tests** (`proptest`): the OSC
   parser never panics on arbitrary bytes.
2. **Router integration tests** (Tauri mock runtime, `tauri::test`): every router
   is exercised through the real invoke pipeline — registration, state extraction,
   argument deserialization — in plain cargo test, no webview. Convention: a new
   router lands with its mock-runtime test in the same PR. (Spec: T1.) On Windows
   this requires `acter-app`'s `build.rs` to embed the app manifest with
   `rustc-link-arg` (not tauri's default `-bins`-scoped link) so test executables
   carry the Common-Controls v6 manifest; without it they abort at load with
   `STATUS_ENTRYPOINT_NOT_FOUND`. Do not revert that build.rs to a bare
   `tauri_build::build()`.
3. **Golden transcript tests** (the workhorse): raw byte captures from real
   PowerShell/cmd/bash sessions committed as fixtures, replayed through the
   term + boundary pipeline, asserting extracted command/output blocks. Catches
   real-world escape-sequence weirdness without spawning shells on every run.
4. **Integration tests**: spawn a real ConPTY, run trivial commands end-to-end.
   Windows CI runner, separate job.
5. **End-to-end tests** (WebdriverIO Tauri service, spec T2): the built app over
   WebDriver — elements located by accessible name, live-region lifecycle asserted
   on real DOM nodes, axe-core injected into the running WebView2. Separate
   non-blocking CI job until stable.
6. **Accessibility, manual**: NVDA pass against the spec's checklist — automation
   cannot hear speech, and here the speech is the product. The checklist and its
   results live in the implementing PR's body as checkboxes; findings inline on
   unchecked items, spawning iteration entries in ROADMAP.md.

## Tooling floor

- `clippy` with warnings denied, `rustfmt` enforced.
- CI on a Windows runner from day one.
