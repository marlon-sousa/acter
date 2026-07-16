# Spec: PR T1 — router integration tests via the Tauri mock runtime

First PR of the testing-infrastructure track (Track T in ROADMAP.md). Ground line
for backend-side integration testing: every router gets an in-process test through
the real Tauri invoke pipeline — no webview, no window, plain `cargo test`.

## Background (investigated 2026-07-16)

- `tauri::test` (crate feature `test`) provides `mock_builder()`, `mock_context()`,
  `noop_assets()`, and `get_ipc_response()`: an IPC request is executed through the
  real registered handler and managed state, exercising command registration, state
  extraction, and argument deserialization — the layer unit tests skip.
- **Known blocker:** a first attempt on Windows compiled but the test binary crashed
  at load with `STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139), before any test ran.
  Root cause not yet identified. Hypotheses to check, in order:
  1. DLL search order for test executables (they run from `target\debug\deps\`,
     unlike the app exe in `target\debug\`).
  2. Feature unification: enabling `tauri/test` as a dev-dependency may change the
     linked feature set of the normal build.
  3. Known upstream issues (tauri GitHub issue tracker) for this exact status code
     with the `test` feature on Windows.

## Deliverables

- `crates/acter-app`: dev-dependencies `tauri` with feature `test` plus
  `serde_json`; a shared test helper that builds the mock app (managed `AppState`
  with the real service wired) and executes an `InvokeRequest` against a named
  command.
- Router tests colocated in `routers.rs` (`#[cfg(test)] mod tests`, per the
  tests-with-code convention):
  - `echo` round-trips through the real router and state.
  - a wrong-argument invoke (e.g. missing `text`) surfaces an error response
    rather than a panic.
- Convention recorded for the future: **every new router lands with a mock-runtime
  test in the same PR** (extends the "component + trait + unit tests" rule to the
  router layer).

## Acceptance criteria

1. The Windows loader crash is root-caused and fixed — `cargo test --workspace`
   passes locally on Windows and in CI.
2. Both router tests above pass.
3. The investigation outcome (cause + fix) is recorded in this spec in the same PR.

## Investigation outcome (2026-07-16) — root-caused and fixed

The crash reproduced immediately: enabling `tauri`'s `test` feature and running
`cargo test -p acter-app` aborted the lib test binary at load with
`0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`, before any test ran.

Root cause: Tauri embeds the Windows application manifest — the one declaring the
`Microsoft.Windows.Common-Controls` v6 dependency — through `tauri-winres` /
`embed-resource`, which links it with `rustc-link-arg-bins`. The `-bins` suffix
scopes the link argument to binary targets only, so **test executables are built
without the manifest**. Without the v6 Common-Controls dependency the loader binds
against the ComCtl5 stub, whose v6 entry points are deliberately absent, and the
process dies with `STATUS_ENTRYPOINT_NOT_FOUND` at startup. This matches upstream
tauri issue #13419 (hypothesis 3); it is a manifest-scoping problem, not DLL search
order (hypothesis 1) or feature unification (hypothesis 2).

Fix (self-contained, no `.cargo/config.toml` or env vars): opt out of Tauri's
manifest with `WindowsAttributes::new_without_app_manifest()` and embed the same
manifest ourselves from `build.rs` using `cargo:rustc-link-arg` (no `-bins`), which
applies to binaries **and** tests. The app binary keeps the identical manifest;
test binaries now get it too. Files: `crates/acter-app/build.rs` and
`crates/acter-app/windows-app-manifest.xml` (byte-for-byte the tauri-build default).
Result: `cargo test --workspace`, `cargo fmt --check`, and
`cargo clippy --workspace --all-targets -D warnings` all pass locally on Windows.
The fallback below was not needed.

## Fallback (explicit downgrade path)

If the loader crash proves unfixable within reasonable effort (upstream bug, no
workaround), this spec is amended to record the finding, the tier is dropped, and
router coverage is reassigned: controllers/services keep their fake-based tests,
and the invoke pipeline is covered by the T2 end-to-end smoke test instead. That
decision must be documented, not silent.

## Out of scope

WebDriver/E2E (T2), frontend `@tauri-apps/api/mocks` usage (arrives with A2/A3
tests when the frontend router grows real event handling).

## Definition of done

Merged with green CI; convention noted in ARCHITECTURE.md test strategy (same PR).
