---
name: roadmap
description: The Acter roadmap process. How to find the next step on the status board, record a finding as a new entry, promote an entry into a spec, and flip an entry to Done at the end of a PR. Use in any planning session, when a finding needs recording, and at the end of every implementing PR.
---

# The roadmap process

The board is [docs/ROADMAP.md](../../../docs/ROADMAP.md): one line per entry, its status,
and either its entry file or its spec. Open entries live one per file under
[docs/roadmap/](../../../docs/roadmap/). Specs live under
[docs/specs/](../../../docs/specs/). An entry is the question; a spec is the answer; the
merged PR is the record. Nothing is written twice.

## Finding the next step

1. Read the "What is next" section of the board. The first line of each lane is that
   lane's next step. Lanes run in parallel with at most one open PR per lane; order
   within a lane is strict unless the lane says otherwise.
2. The line's Spec field says what kind of work comes next. "Spec: none yet" means a
   spec conversation with the user, never code. A spec path means implement it.
3. Open the entry file for the one entry being decided, and nothing else on the board.

## Recording a finding

1. Add a line to the lane's list on the board, numbered as a sub-entry of the entry
   whose work surfaced it (28.12 under 28), or as the lane's next integer for new work.
   The line is: number, `**Open**`, one sentence naming the entry, `Entry:` link, and
   `Spec: none yet.`
2. Create `docs/roadmap/<number>-<slug>.md` with a heading of number and title, the lane,
   and then the finding: what was observed, on what, and when, in the words of the
   person who observed it. Measured facts go here with their date. Do not write what
   should be done about it; that is the spec's job.
3. Repeat the line under "What is next" in its lane.

## Promoting an entry into a spec

1. Agree the spec in conversation. Its opening sections restate the entry and carry
   every measured fact from the entry file, so nothing is lost when the file goes.
2. In the implementing PR: add the spec under `docs/specs/`, delete the entry file, and
   change the board line's `Entry:` link to `Spec:` with the spec path. The line stays
   Open until the PR merges.

## Flipping to Done

At the end of the implementing PR, change the line's `**Open**` to `**Done**` in both
the lane list and "What is next" (remove it from "What is next"). Write nothing else:
what shipped is the spec plus the PR. The mark becomes true on main exactly when the
user merges.

## Closing without a spec

An entry answered by a measurement, superseded, or folded into another entry gets
`**Closed <date>**` or `**Answered**` on its line with one clause saying why, and its
entry file stays, because that file is then the only record. If the answer is a product
fact, move it into DESIGN.md and delete the file instead.

## Lane histories

The files named `lane-*.md` and `keyboard-routing.md` under `docs/roadmap/` are the
archive of entries that were Done before entries had files. They are being retired by
lane 4 entry C12 and are not where new writing goes.
