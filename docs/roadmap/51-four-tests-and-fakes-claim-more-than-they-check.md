# 51 — Four tests and fakes claim more than they check.

Lane: lane-2-domain. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**Four tests and fakes claim more than they check.** Spec: none yet → specify first.

Found on 2026-09-23 while stripping comments for lane 4 (PRs #66, #67, #69 and #70). Each
was a comment saying what a test or a fake guarantees; the comments are gone, and what
they claimed is not true of the code.

- **A new connection's missing origin is untested.**
  `ui/test/adapters/new_connection_dialog.test.ts` records each attempt's origin in
  `origins`, and its header said the added behaviour is that a new connection carries no
  origin. No test asserts anything about `origins`.
- **The in-memory host-key store keeps duplicates.** `RememberedHostKeys` in
  `crates/acter-core/src/ports/driven/host_key_store.rs` was documented as behaving like
  the real store. `Settings::accept` in `crates/acter-app/src/adapters/settings_file.rs`
  skips a key already recorded for that host, port and fingerprint; the fake always
  pushes, so a test accepting one key twice sees two records where the product keeps one.
- **The scripted transport's second start and its interrupt order.** In
  `crates/acter-transports/src/scripted.rs`, `Transport::start` on an already-started
  transport stores the new sender before returning, so the second receiver stays open and
  silent rather than closing. The test
  `starting_twice_ends_the_second_session_rather_than_forking_the_far_end` checks only
  `try_recv` and a `Closed` write, so it does not notice. Separately, for one write such as
  `"a\n\x03"`, the interrupt is answered before `a`, though `a` arrived first.
- **A call in a connect-service test has no effect.** In
  `crates/acter-core/src/services/connect.rs`, the test
  `saving_under_its_own_origin_replaces_rather_than_refusing` first calls
  `use_profile(Terminal, Some("work")).ok()`, which cannot start on the fake Windows
  machine and whose result is ignored, before connecting with `cmd`.
