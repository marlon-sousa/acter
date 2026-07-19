# Spec: PR A2 — IPC protocol types + TypeScript generation

First PR of the UI protocol track after the testing ground line (T1/T2). It turns
the **Decided** IPC protocol in ARCHITECTURE.md ("IPC protocol and communication
flow") into concrete `acter-core` types, generates their TypeScript mirror from the
single Rust source, and locks both against drift. No behavior ships: this is the
wire contract that A3's fake backend and every later UI PR are written against.

## Why now / relation to the roadmap

- Roadmap lane 1, entry A2. Scope sketch: "IPC event/command types as entities in
  acter-core, tauri-specta TypeScript generation, serde round-trip tests."
- Guardrail from DESIGN.md (Phasing #2): **the protocol is designed for both modes,
  implemented as a subset.** `resize`, `alt-screen-entered/left`, connection state,
  and mode are defined now even though Phase 1 barely produces them, so Phase 2 is
  purely additive. A2 defines the full surface as *types*; A3 wires producers for
  the Phase-1 subset only.
- The types are pure data (`entity/value` role). No policy, no I/O, no Tauri — they
  belong in `acter-core`, which stays free of both.

## Design decisions to confirm in review

These are the calls this spec makes; they are the reviewable surface, since the
spec is agreed before any code lands.

1. **Types-only generation, invoke wrappers stay hand-written.** ARCHITECTURE.md
   already Decides that the frontend router "implements the `BackendApi` interface
   as typed invoke wrappers" (hand-written) and that only the *types* are generated.
   So A2 generates a **runtime-free** `.ts` module — pure `export type` declarations,
   zero `@tauri-apps/api` import — using the `specta` + `specta-typescript` exporter
   (the type engine underneath tauri-specta). This keeps `routers/tauri.ts` the sole
   importer of `@tauri-apps/api` (an ARCHITECTURE invariant) and avoids emitting
   command/event runtime helpers we have decided not to use. ARCHITECTURE.md's
   wording ("generated with tauri-specta") is refined to "the specta/tauri-specta
   type exporter" in this same PR — a proposed, non-silent change, not a reversal:
   same toolchain, types layer only.
2. **Events carry no `session_id`.** Per ARCHITECTURE, the event stream is a
   per-session Tauri Channel established at attach; session identity is the channel,
   not the payload. So `SessionEvent` variants omit `session_id`. Commands (invokes,
   not in this PR's producer set) will carry `session_id` as an argument.
3. **Internally-tagged event enum.** `SessionEvent` serializes with
   `#[serde(tag = "type")]`, so specta emits a discriminated union keyed on `type` —
   the frontend compiler can force exhaustive handling. The tag key `"type"` is
   asserted by a round-trip/shape test so it cannot drift silently.
4. **Correlation and identity are transparent newtypes.** `CommandId(u64)`,
   `SessionId(u64)`, `ExitCode(i32)` are `#[serde(transparent)]` newtypes — wire
   shape is the bare scalar, but the Rust and TS types are distinct, so a command id
   can never be passed where a session id is wanted.
5. **Completion and snapshot types are out of scope** (see below) — their shapes are
   genuinely undecided and belong to the domains that build them (A4 completion, A3
   session model). A2 defines the streaming I/O + event protocol only.

## Protocol surface (the types this PR defines)

All types derive `serde::{Serialize, Deserialize}` and `specta::Type`, live in
`acter-core`, and are re-exported from its `lib.rs` facade.

### Shared value types — `entities/protocol_common.rs`

- `SessionId(u64)` — transparent newtype; identifies a session/tab (tabs are
  Decided; even the single Phase-1 session has an id).
- `CommandId(u64)` — transparent newtype; the correlation id ARCHITECTURE requires
  (`submit_command` returns it; every event about that command carries it).
- `ExitCode(i32)` — transparent newtype; nonzero is a failure, announced distinctly.
- `ReadMode` — the read verdict computed backend-side by the pacing policy and
  carried on announcement-bearing events: `Auto | TooBig | Quiet`.
- `Mode` — rendering mode: `NonInteractive | Interactive` (Phase 1 only ever emits
  `NonInteractive`; the variant is defined so Phase 2 is additive).
- `ConnectionState` — `Connected | Reconnecting | Disconnected` (local transport is
  always `Connected` in Phase 1; SSH exercises the rest later).

### Command return payloads — `entities/protocol_commands.rs`

The Phase-1 invoke surface is thin; most arguments are primitives (`session_id`,
`line`, `cols`, `rows`) passed straight to routers in A3. The one named payload A2
defines is the correlation ack:

- `SubmitAck { command_id: CommandId }` — the immediate return of `submit_command`
  (invoke never waits on the shell; it acks with the id all later events carry).

Invoke *argument* structs are not introduced here — A3 adds the routers and their
plain arguments. Naming/return types for `request_completion` (A4) and
`get_session_snapshot` (A3, needs the session model) are deferred to those PRs.

### Event envelope — `entities/protocol_events.rs`

`SessionEvent`, `#[serde(tag = "type")]`, one envelope down the per-session channel:

- `CommandStarted { command_id: CommandId }`
- `Output { command_id: CommandId, text: String, read_mode: ReadMode }` — a coalesced
  quiescent chunk; carries the read verdict so the frontend obeys, never re-measures.
- `CommandFinished { command_id: CommandId, exit_code: ExitCode, read_mode: ReadMode }`
- `CommandStillRunning { command_id: CommandId }` — the patience announcement
  ("long command running, output accumulating"); defined now, produced by the pacing
  policy later.
- `AltScreenEntered` / `AltScreenLeft` — Phase-1 alt-screen detection (DESIGN
  Phasing #3) rides these.
- `TitleChanged { title: String }`
- `ConnectionChanged { state: ConnectionState }`

Producers for `Output` / `CommandStarted` / `CommandFinished` arrive with A3's fake
backend; the alt-screen, title, still-running, and connection variants are defined
now and produced when their sources land. This is the "designed for both modes,
implemented as a subset" guardrail made concrete.

## Deliverables

### `acter-core`
- `Cargo.toml`: add `serde` (feature `derive`) and `specta` (with its serde-compat
  feature) as normal dependencies. Both are type/derive crates — no I/O, no Tauri —
  so the "zero Tauri, zero I/O" rule holds.
- `lib.rs`: facade `mod` + `pub use` re-exports of every protocol type.
- `entities.rs` + `entities/`: `protocol_common.rs`, `protocol_commands.rs`,
  `protocol_events.rs`. Each file's `//!` first line declares role `entity/value`.
  (Adds `protocol_common.rs` to ARCHITECTURE's reference layout — recorded there in
  this PR.)
- Inline `#[cfg(test)] mod tests` per file:
  - **serde round-trip** for every value type and every `SessionEvent` variant
    (serialize → deserialize → `assert_eq!`).
  - **wire-shape locks:** `SessionEvent::Output` serializes to an object whose
    `"type"` is `"Output"` and whose `command_id` is the bare integer (transparent
    newtype); at least one assertion per transparent newtype confirms the scalar wire
    form. These fail loudly if a serde attribute is dropped.

### `acter-app` (generator + drift guard)
- `Cargo.toml`: add `acter-core`, `specta`, and `specta-typescript` as
  **dev-dependencies** (only the export test uses them until A3 makes `acter-core` a
  normal dependency).
- `tests/protocol_bindings.rs`: an integration test that builds a
  `specta::TypeCollection` over the protocol types and writes
  `../../ui/src/protocol.ts` via `specta_typescript::Typescript` with a
  "GENERATED — do not edit; regenerate with `cargo test -p acter-app
  --test protocol_bindings`" banner. The test is the generator (canonical
  tauri-specta pattern); the committed file is the artifact.

### Frontend
- `ui/src/protocol.ts`: **generated, committed.** Pure `export type` declarations
  (the discriminated union + value types + `SubmitAck`), no runtime, no
  `@tauri-apps/api` import. Header banner marks it generated.
- `ui/test/protocol.test.ts` (vitest): a compile-time exhaustiveness guard — a
  `switch` over `SessionEvent["type"]` with an `assertNever(x: never)` default, so
  adding a variant to the Rust source without handling it fails `tsc`. Also one
  runtime assertion that a hand-built `Output` value satisfies the generated type.
  This is the guard A3's inbound channel router builds on.
- Echo harness (`routers/tauri.ts`, `ports/backend_api.ts`, the Rust echo
  service/router) is **untouched** — A3 replaces it with the real session domain.

### CI (`.github/workflows/ci.yml`)
- In the existing `checks` job, after `cargo test --workspace`, add a
  **"Protocol bindings up to date"** step: re-run the generator test, then
  `git diff --exit-code ui/src/protocol.ts`. A stale committed binding fails the
  build. (Deterministic because specta emits in declaration order.)

### Docs
- `ARCHITECTURE.md`: refine the generation wording per decision 1; add
  `protocol_common.rs` and the `ui/src/protocol.ts` generated-artifact location to
  the reference layout.
- `ROADMAP.md`: this PR flips lane-1 entry A2 to `Done (PR #n, date)` and sets its
  Spec field to `[a2-protocol.md](specs/a2-protocol.md)` — per the process now on
  main, the spec lands in this PR, not ahead of it.

## Acceptance criteria

1. Every protocol type exists in `acter-core`, derives serde + `specta::Type`, and is
   re-exported from `lib.rs`; `cargo test --workspace` passes on Windows and CI.
2. Round-trip and wire-shape tests pass for every value type and every `SessionEvent`
   variant, including the `"type"` tag and transparent-newtype scalar assertions.
3. `ui/src/protocol.ts` is committed, is byte-identical to a fresh generation (CI
   drift step green), imports nothing, and `npm run typecheck` passes over it.
4. `ui/test/protocol.test.ts` passes and genuinely fails `tsc` if a `SessionEvent`
   variant is added to the Rust source but not handled (verified by a throwaway local
   edit during development, reverted before merge — noted in the PR body).
5. The echo harness and all T1/T2 tests still pass unchanged.

## Out of scope

- **Producers / wiring.** No router emits or consumes these types yet; A3's fake
  backend is the first producer.
- **Completion protocol** (`request_completion` payloads) — A4.
- **`get_session_snapshot` / `SessionSnapshot`** — needs the session model; A3.
- **Invoke argument structs** for `submit_command` / `write_input` / `resize` /
  `toggle_mode` — added with their routers in A3 (primitive args + `SubmitAck`).
- **tauri-specta command/event runtime helpers** — deliberately not generated
  (decision 1); if ever wanted, an additive change.

## Definition of done

Merged with green CI (including the new drift step and the E2E job); the protocol
types and their committed TypeScript mirror are on main; ARCHITECTURE.md and
ROADMAP.md updated in this PR; no accessibility checklist (this PR ships no
user-facing surface — noted in the PR body so the checklist gate is satisfied).

## Manual accessibility checklist

None — A2 adds no user-facing UI or announced string. The PR body states this
explicitly so the unchecked-checkbox merge gate has nothing to block on.
