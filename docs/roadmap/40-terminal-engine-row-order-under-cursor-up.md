# 40 — The terminal engine can emit two rows in the wrong order after the cursor moves up.

Lane: lane-2-domain. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**The terminal engine can emit two rows in the wrong order after the cursor moves up.**
Spec: none yet → specify first.

Found on 2026-09-23 by CI on PR #66 (lane 4, C3), in the `fmt, clippy, test (macOS)` job.
That PR changed only comments in `acter-core`, and `acter-term` is untouched on its branch.
Two property tests in `crates/acter-term/src/alacritty_engine.rs` failed on a randomly
generated transcript. The rerun of the same commit drew new input and passed, and main had
been green in every run before. The bug is in the code on main and was not hit until then.

- `no_line_is_ever_lost` failed with seed
  `cc 7f868fe05335bfc4803d7306e084d8396a7935d7460d4c00282f273c9f3b1a02`. Replaying the
  seed locally on Windows through the workspace build
  (`cargo test --workspace --lib alacritty_engine::tests`) fails every time, with the
  same assertion: replaying the emitted items gives `["a", "b"]`, while the reference
  terminal gives `["b", "a"]`. The minimal failing input, as proptest printed it,
  with `chunk = 1`:
  `[13, 10, 27, 91, 66, 97, 9, 27, 91, 51, 88, 27, 91, 65, 27, 91, 51, 49, 109, 27, 91,
  75, 27, 91, 65, 97, 97, 97, 32, 98, 32, 32, 27, 91, 65, 27, 91, 50, 75]`. That is
  `CR LF`, `ESC[B`, `a`, `TAB`, `ESC[3X`, `ESC[A`, `ESC[31m`, `ESC[K`, `ESC[A`,
  `aaa b  `, `ESC[A`, `ESC[2K`.
- `reconstruction_is_independent_of_chunking` failed in the same run: chunked replay
  gives `[" a", "   a"]` and the whole-transcript replay gives `["   a", " a"]`. Its
  minimal failing input: `[13, 10, 27, 91, 66, 32, 97, 32, 27, 91, 65, 97, 27, 91, 65, 97,
  97, 97, 97, 97, 32, 97, 32, 32, 27, 91, 65, 27, 91, 75]`, which is `CR LF`, `ESC[B`,
  ` a `, `ESC[A`, `a`, `ESC[A`, `aaaaa a  `, `ESC[A`, `ESC[K`. The log did not show this
  test's own seed.

- Again on 2026-09-23, in the `fmt, clippy, test (Windows)` job of PR #67 (C4, comments
  only): `no_line_is_ever_lost` with seed
  `cc 95462c408b57ea8b7c66954fec5ed923db1ec8210789bb8bf13302d6580fa302`, replay gives
  `["a", "c"]` where the reference gives `["c", "a"]`. The minimal input begins `ESC[2K`,
  `ESC[31m`, `ESC[31m`, `ESC[5G`, `aaa aa ac`, `ESC[K`, `ESC[B`, `CR`, `ESC[31m`, `ESC[K`,
  `a`, `ESC[A`, `ESC[A`; the log was cut off after that. The rerun passed. That is two of
  the four CI runs of the day.

All of these inputs move the cursor up past rows already written, then write or erase there.
The seed is not in `crates/acter-term/proptest-regressions/alacritty_engine.txt`, so CI
only hits this again by chance.
