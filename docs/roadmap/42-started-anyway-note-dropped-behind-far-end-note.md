# 42 — A file started anyway goes unmentioned when the far end also has a note.

Lane: lane-2-domain. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**A file started anyway goes unmentioned when the far end also has a note.** Spec: none
yet → specify first.

Found on 2026-09-23 while stripping comments for C4 (PR #67), by reading the code; nothing
has been observed with a screen reader yet.

In `crates/acter-core/src/services/connect.rs`, `use_profile` builds two optional clauses
for the connection announcement:

- `agreed`, from `verified`: the "started although …" clause that `Verdict::note` gives
  for a file that did not verify and that the user chose to start anyway;
- `note`, from the factory's `Started`: what the far end is, from `far_end_note` in
  `crates/acter-app/src/container.rs`.

It keeps one: `let note = note.or(agreed);`. The comment above that line, deleted in C4,
said at most one of the two is ever present. That is false. `far_end_note` gives a note
for SSH, for WSL (`container.rs`, `fn wsl`) and for a Unix login shell (`fn unix_shell`)
whenever the shell was detected. WSL starts `wsl.exe`, and the macOS Terminal kind starts
a login shell, and both are files `verified` checks. When such a file did not verify and
the user chose Start anyway, the announcement says what the far end is and says nothing
about the file having been started unverified.
