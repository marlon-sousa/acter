# 40 — a line that appears above live lines keeps its place

Roadmap entry 40, lane 2. The fix rides in PR #71, as the user asked on 2026-09-23.

## What was observed

The CI property tests in `crates/acter-term/src/alacritty_engine.rs` failed on randomly
generated transcripts, on PRs that did not touch `acter-term`. Each rerun drew new input and
passed.

- **PR #66, macOS job.** `no_line_is_ever_lost`, seed
  `cc 7f868fe05335bfc4803d7306e084d8396a7935d7460d4c00282f273c9f3b1a02`: the replay gave
  `["a", "b"]` where the reference terminal gave `["b", "a"]`. The minimal input, with
  `chunk = 1`, was `CR LF`, `ESC[B`, `a`, `TAB`, `ESC[3X`, `ESC[A`, `ESC[31m`, `ESC[K`,
  `ESC[A`, `aaa b  `, `ESC[A`, `ESC[2K`. It failed every time when replayed locally on
  Windows. `reconstruction_is_independent_of_chunking` failed in the same run: chunked
  `[" a", "   a"]` against whole `["   a", " a"]`, from `CR LF`, `ESC[B`, ` a `, `ESC[A`,
  `a`, `ESC[A`, `aaaaa a  `, `ESC[A`, `ESC[K`.
- **PR #67, Windows job.** `no_line_is_ever_lost`, seed
  `cc 95462c408b57ea8b7c66954fec5ed923db1ec8210789bb8bf13302d6580fa302`: `["a", "c"]` against
  `["c", "a"]`.
- **PR #71, Windows job.** `reconstruction_is_independent_of_chunking`, seed
  `cc 67b21f56d1ea70dd752c7a5793451e845233c2b165012986195495f579b19305`: chunked
  `["a", "b", "a"]` against whole `["a", "a", "b"]`, from `aaa aaa  aaaa`, `ESC[A`, `CR LF`,
  `CR LF`, `b`, `ESC[A`, `ESC[A`, `ESC[K`.

Every input moves the cursor up past rows already written, then writes or erases there.

## The cause

The extractor mints a `LineId` the first time it finds a line, and a reader places each id
where it first appeared. The frontend buffer appends a row for a new id, and the tests'
`replay` does the same. A line that appears above lines already emitted therefore lands
below them. There are two ways this happens: the cursor goes up and writes on a blank row,
or an erase splits a wrapped line in two, so that its tail becomes a line of its own. In the
PR #71 input, `ESC[K` removes the wrap from the first row. Its tail, `a`, becomes a new line
above `b`, gets a new id, and is shown after `b`.

This affects users as well as the tests: the Results buffer shows the rows in the wrong
order.

## Decisions

1. **A new line above live lines takes over the first live line's id, and every id below
   moves down one line.** The bottom line gets a fresh id, announced at once with an empty
   `Appended`, so its place is fixed before anything else is minted. Each id keeps its screen
   position. The text follows through the existing revisions: the extractor records what the
   reader currently shows for each moved id, and the next scan emits `Appended`, `Rewritten`
   or nothing against that.
2. **Only live lines move.** A line whose id has settled (after a block-closing marker) keeps
   it, because nothing may follow a settled id. A new line written above a settled line on
   screen therefore still appears after it. That belongs to the next block, which is where a
   reader expects it.
3. **Every recorded input becomes a test that runs every time,** and all three seeds go into
   `proptest-regressions/alacritty_engine.txt`, so CI no longer meets them only by chance.

## Files touched

- `crates/acter-term/src/alacritty_engine/extractor.rs`
- `crates/acter-term/src/alacritty_engine.rs` (three tests)
- `crates/acter-term/proptest-regressions/alacritty_engine.txt`

## Acceptance criteria

1. The three recorded inputs, as `#[test]`s at chunk sizes 1, 3, 7, 29 and whole, match the
   reference grid. Measured: all three fail on the old extractor and pass on the new one.
2. `cargo test --workspace` passes, including the three seeds.
3. `no_line_is_ever_lost` and `reconstruction_is_independent_of_chunking` pass with
   `PROPTEST_CASES=20000`.
