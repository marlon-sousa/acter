# C — comments say what the code cannot

Roadmap lane 4. One rule in CLAUDE.md, eleven comment-only PRs, and one docs PR series that bring the
existing code under it. This file is the whole contract for the lane: the rule, the
rubric a worker applies line by line, the procedure for one file, the PR list with the
files each one touches, and the definition of done. A worker on any PR of this lane
needs this file, CLAUDE.md and ARCHITECTURE.md, and nothing else.

## What is true today

Measured on 2026-09-14 with `python scripts/comment_ratio.py`, every tracked Rust and
TypeScript file except the generated `ui/src/protocol.ts`:

- 39,709 code lines and 18,314 comment lines, or 0.46 comment lines per code line.
- Every file except the `ui/src` sources was read in full against the rubric below. About
  80 percent of the comment lines fall in the delete buckets. `ui/src` was sampled and
  follows the same pattern.
- The generated `ui/src/protocol.ts` carries another 836 comment lines. They are copies of
  Rust doc comments and regenerate from them, so no PR edits that file by hand.

Where the deleted material came from, in agent-read files: rhetoric and bold theses
5,321 lines, restatements of the code 2,681, test narration 1,732, history 1,090, spec
and decision citations 781, architecture re-justified per file 476, rejected
alternatives 412.

Why it is worth eleven PRs rather than leaving it: the duplicated rationale is already
wrong in a dozen places (listed under "stale comments" below), every spec cited in code
already holds the same text, and a file where half the lines are prose is a file no
reader, human or agent, can hold in one view.

## The rule — **Decided**

The rule is in CLAUDE.md under hard rules and is repeated here so this file stands alone.

A comment is one of four things:

1. The module role line required by the module role rule.
2. A measured fact: what was measured, against which program and version, and the
   value. Example: "NVDA 2026.1.1 drops a message queued less than 100 ms after the
   previous one." No date: a rewrite resets what git blame reports, and a version is
   what a re-measurement needs.
3. An invariant or hazard the code does not enforce. Example: "Drop the writer before the
   reader or ConPTY never closes the pipe." Example: "SAFETY: the handle is owned by this
   struct and closed exactly once in Drop."
4. What an absent value, an error or a rejection carries. Example: "None means the
   window has no session." Example: "Rejects with a whole spoken sentence."

One sentence, at the point of use, once in the repository. If the same fact is needed in
two places, the second says "see X" with a path.

Never in code: why a design was chosen, what it replaced, when it changed, what was tried
and rejected, and citations of specs, decisions or roadmap entries. Those go in the spec
and in the PR body. No doc paragraph on a field, parameter, method or test whose name
already says it. No bold in comments.

## The rubric a worker applies

Read each comment paragraph and put it in exactly one bucket. Delete buckets are named
N, keep buckets K.

Delete:

- N1 citation. Any "(spec 26, decision 19)", "roadmap 23.7", "A9 decided", "DESIGN's
  reliability case 2". Delete the citation; if the sentence around it is a K fact, keep the
  sentence without the citation.
- N2 history. "Since B4.4", "until 2026-08-26", "used to", "A9 shipped believing",
  "renamed from", "this sentence said the opposite for five entries". Delete.
- N3 rejected alternative. "Rather than", "not X because", "the other way would have".
  Delete, unless the sentence is the only place a hazard is stated, in which case keep the
  hazard in one sentence with no alternative named.
- N4 restates the code. A paragraph on a field, parameter, variant, method or import that
  says what its name and type already say. Delete the whole paragraph.
- N5 architecture justification. Why ports are separate from adapters, why a service sees
  only ports, why a controller is thin. ARCHITECTURE.md says it once. Delete.
- N6 rhetoric. Bold theses, "the point is", "which is the whole reason", "honesty
  applies", cross-references to other files' reasoning, second and later restatements of
  a fact already stated. Delete.
- N7 test narration. A doc comment above a test that paraphrases the test name or walks
  through the assertions. Delete. A test's name is its documentation; if the name cannot
  carry the intent, rename the test rather than describe it.

Keep, compressed to one sentence each:

- K1 role line. Keep the first line of the `//!` doc or the `// Role:` line. Delete the
  paragraphs after it unless they are K2, K3 or K4.
- K2 measured fact. Keep what was measured, against which program and version, and the
  value. No date: a rewrite resets what git blame reports, and a version is what a
  re-measurement needs. Drop the story of how it was found.
- K3 invariant or hazard. Keep. These are usually already one line inside a body: SAFETY
  notes, drop order, cancel-safety, a field kept alive for its Drop, a call that must
  happen on the main thread.
- K4 what an absent value or an error carries. One line.
- K5 test intent, only when the test name cannot carry it. One line, and prefer renaming.
- K6 generated-file and tooling headers. Keep unchanged.

The tie-break: could a competent engineer with ARCHITECTURE.md and DESIGN.md open the
code and work this out in a few minutes? If yes, delete. If genuinely unsure, keep the
one sentence that states the fact and delete the rest.

Compression means rewriting a kept paragraph as one plain sentence. The kept sentence
states the fact and nothing about who decided it or when it changed.

## Stale comments

The audit found comments that contradict the code or sit on the wrong item. The worker
whose PR touches the file handles it as follows: a stale N comment is deleted like any
other; a stale K comment is corrected to what the code does. Where a comment says the
code does one thing and the code does another, and the comment describes the better
behaviour, do not change the code. Delete or correct the comment and add one line to the
PR body under "found while stripping", so it can become a roadmap iteration entry.

- `crates/acter-core/src/controllers/session_actor.rs` line 69: orphan doc above
  `PromptDrawn`; `CommandEnded` at line 81 is undocumented.
- `crates/acter-core/src/ports/driven/clock.rs` lines 44 to 45: says a dropped sender
  means the timer never fires; the map call resolves immediately.
- `crates/acter-transports/src/ssh/known_hosts.rs` line 156: "into Acter's own file" is
  stale after spec 26.
- `crates/acter-transports/src/ssh/transport.rs` line 327: "asked only when the server
  will take one"; the code asks unconditionally.
- `crates/acter-core/src/services/session.rs` lines 4252 and 4763: module docs for
  modules that were moved out from under them.
- `crates/acter-core/src/entities/connection_kind.rs` lines 116 to 134: describes
  `instructions()` but is attached to `editions()`.
- `ui/test/adapters/new_connection_dialog.test.ts` lines 403 and 451: JSDoc asserts the
  opposite of what the test checks.
- `ui/test/controllers/app.test.ts` line 304: orphan field doc; line 1535: "Ctrl+C is the
  only key reported" is no longer true.
- `ui/test/adapters/window_chrome.test.ts` lines 88 to 92 and
  `ui/test/adapters/announcer.test.ts` lines 203 to 214: paragraphs attached to the wrong
  test.

Line numbers are as of 2026-09-14 and will drift as earlier PRs land; search for the
quoted words.

## Procedure for one file

1. Run `python scripts/comment_ratio.py <file>` and note the line.
2. Read the file top to bottom. For each comment paragraph, name its bucket, then delete
   or compress. Do not skip a paragraph because it is long; the long ones are the point.
3. Touch nothing that is not a comment. No renames, no reformatting by hand, no import
   ordering, no "while I was here". A diff that changes a non-comment line fails review,
   with two exceptions. A test whose narration is deleted may be renamed so the name
   carries the intent, and only the name changes. And `cargo fmt` may reflow code after a
   deletion: once one variant of an enum is a single line with no doc, rustfmt expands
   every struct variant in it onto several lines. A doc comment is never kept only to hold
   the layout still.
4. Run the script again. The file's ratio should be at or under 0.20 comment lines per
   code line. A file above that is allowed only when every remaining comment is K2 or K3,
   and the PR body says so for that file.
5. Run the gate for the PR (listed per PR below). Comments cannot change behaviour, so a
   red gate means a non-comment line moved; find it and revert it. The script's code
   count for the file must be identical before and after, except for what `cargo fmt`
   reflowed; that is the reviewer's first check.

The `///` doc comments become `pub` API docs on rustdoc and Specta copies the ones on IPC
types into `ui/src/protocol.ts`. Both are fine to shrink: a `pub` item with a name that
says what it is needs no doc line at all, and a missing doc is not a build error in this
workspace.

## The PRs

Every PR in this lane is comment-only, one reviewable unit, and independent of the others. The
order below goes from the files with the most measured facts to keep, which calibrate the
worker's judgement under review, to the largest files. A worker who has done C1 and C2
under review can do the rest without one.

Each PR body carries: the script's total line for the touched files before and after; a
"found while stripping" list, possibly empty; and the sentence "no non-comment line
changed" or the list of test renames and of the files `cargo fmt` reflowed.

### C1 — `acter-core` policies

Files: every file under `crates/acter-core/src/policies/` plus
`crates/acter-core/src/policies.rs`. The rule, this spec, the script and the
`strip-comments` skill landed on main ahead of the lane on 2026-09-14, so that no PR is
stripped before the rule that governs it exists.

Audit figures: 875 comment lines, about 700 to delete. This group has the highest share
of measured facts (K2) of any batch, which is why it goes first: the reviewer sees the
worker's keep judgement on the hardest cases.

Gate: `cargo test -p acter-core`.

### C2 — `acter-core` entities and the crate's root files

Files: every file under `crates/acter-core/src/entities/`, plus `entities.rs`,
`services.rs`, `ports.rs`, `controllers.rs` and `lib.rs` in `crates/acter-core/src/`.

Audit figures: 1,784 comment lines, about 1,240 to delete. This is the batch with the
most K4 lines ("what None means"), most of which are one-liners on protocol types that
Specta copies to the frontend. Keep those as single lines.

Gate: `cargo test -p acter-core`, then `cargo test -p acter-app --test protocol_bindings`,
which rewrites `ui/src/protocol.ts`; commit the regenerated file in the same PR.

### C3 — `acter-core` ports and controllers

Files: every file under `crates/acter-core/src/ports/` and
`crates/acter-core/src/controllers/`.

Audit figures: 1,590 comment lines, about 1,360 to delete. `ports/driven/this_computer.rs`
is 59 percent comment and is the model case of the template this lane removes. Stale
items: `session_actor.rs` and `clock.rs` from the list above.

Gate: `cargo test -p acter-core`.

### C4 — `acter-core` services: connect and conversation

Files: `crates/acter-core/src/services/connect.rs` and
`crates/acter-core/src/services/conversation.rs`.

Audit figures: 784 comment lines, about 700 to delete. The test section of `connect.rs`
has 267 comment lines of which 9 are keepable.

Gate: `cargo test -p acter-core`.

### C5 — `acter-core` services: session

Files: `crates/acter-core/src/services/session.rs` alone.

Audit figures: 1,891 comment lines, about 1,560 to delete. The non-test part keeps about
260 lines, almost all K3 inside the pump and drop paths; the test section keeps about 60.
Stale items: the two moved module docs from the list above.

Gate: `cargo test -p acter-core`.

### C6 — `acter-shells`

Files: every file under `crates/acter-shells/src/`.

Audit figures: 2,226 comment lines, about 1,750 to delete. This crate has the most
measured platform facts in the repository (PSReadLine under screen readers, busybox PS1,
WSL distribution listing, Windows signature trust). Keep each once, one sentence, naming the program and version.

Gate: `cargo test -p acter-shells`.

### C7 — `acter-transports` sources

Files: every file under `crates/acter-transports/src/` and
`crates/acter-transports/examples/`.

Audit figures: 1,175 comment lines, about 940 to delete. Stale items: `known_hosts.rs`
and `transport.rs` from the list above.

Gate: `cargo test -p acter-transports --lib`.

### C8 — `acter-transports` tests

Files: every file under `crates/acter-transports/tests/`.

Audit figures: 1,511 comment lines, about 1,280 to delete. Keep the K2 facts about what
ConPTY and the real shells do; they are the only record of several flaky-test causes.

Gate: `cargo test -p acter-transports`. The real-session tests need a shell installed;
run what the machine can run and say in the PR body which tests ran.

### C9 — `acter-app` and `acter-term`

Files: every file under `crates/acter-app/` and `crates/acter-term/`, including
`build.rs` and the `tests/` folders.

Audit figures: 1,815 comment lines, about 1,510 to delete. `container.rs` alone has 737
comment lines of which 87 are keepable, mostly the Tauri main-thread and drop-order
hazards; keep each once here and delete the copies elsewhere. `acter-term` keeps a higher
share, around 30 percent, because the vte handler notes are measured facts.

Gate: `cargo test -p acter-app` and `cargo test -p acter-term`.

### C10 — frontend sources

Files: every `.ts` file under `ui/src/` except `ui/src/protocol.ts`.

Audit figures: 2,743 comment lines, about 2,200 to delete (sampled, not read in full).
`controllers/app.ts` has 501 comment lines; the pinned announcement strings at its top
keep one line each saying they are spoken and therefore a domain requirement, and nothing
about which spec or entry chose the words. `adapters/announcer.ts` keeps its measured NVDA
constants with the NVDA version they were measured against.

Gate: `npm run typecheck` and `npm test` in `ui/`.

### C11 — frontend tests and end-to-end tests

Files: every `.ts` file under `ui/test/` and `e2e/`.

Audit figures: 1,868 comment lines, about 1,450 to delete. The jsdom dialog shim is
explained in ten files; keep one sentence in the helper that installs it and delete the
rest. Stale items: the four `ui/test` entries from the list above.

Gate: `npm run typecheck` and `npm test` in `ui/`; the e2e suite runs only where a
WebDriver is installed, so say in the PR body whether it ran.

### C12 — retire the lane histories

Docs only, one lane file per PR, under the rubric above: keep the measured fact in the
spec or DESIGN.md, drop the story. The entry file
[c12-retire-the-lane-histories.md](../roadmap/c12-retire-the-lane-histories.md) carries
the procedure.

## Definition of done for the lane

- Every PR above merged; the roadmap lane 4 entries flipped to Done by their own PRs.
- `python scripts/comment_ratio.py` with no arguments reports at or under 0.15 comment
  lines per code line for the repository.
- No comment in the repository cites a spec, decision or roadmap entry; check with
  `rg -n "spec [0-9A-Z]|decision [0-9]|roadmap [0-9]" --type rust --type ts` and expect
  only hits in `docs/`.
- Every stale comment listed above is gone or corrected, and every one that revealed a
  code question has an iteration entry in ROADMAP.md.
