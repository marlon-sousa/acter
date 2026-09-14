# C12 — retire the lane histories

Lane: lane-4-comments. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry
until a spec absorbs it; the spec's PR deletes it.

On 2026-09-14 the roadmap was split: the board became one line per entry, open entries
became one file each in this folder, and the bodies of every entry that was already Done
were moved verbatim into `lane-1-ui.md`, `lane-2-domain.md`, `lane-3-macos.md` and
`keyboard-routing.md`. Those four files are an archive, not a working document. Under
the roadmap process an entry that has a spec and is Done keeps no body: what shipped is
the spec plus the merged PR.

What is measured: the four files hold 4,458 lines. Most of it is what shipped and why,
which the specs and PRs already say. Some of it is dated measurements and user
observations that spawned later entries, and a few of those may exist nowhere else.

The work, one lane file per PR, with the strip-comments rubric's judgement:

1. For each Done entry in the file, list every dated measurement and every quoted user
   observation in its body.
2. For each, check whether the spec the board line links to, or DESIGN.md, already
   states it. If it does, nothing to do. If it does not, add one sentence to the spec's
   "what was measured" section, or to DESIGN.md when it is a product fact.
3. Delete the entry's body from the lane file.
4. An entry marked Closed or Answered with no spec keeps its body: move it to its own
   entry file named by number and slug, and point the board line at it with `Entry:`.
5. When the lane file is empty, delete it and remove the `History:` line from that
   lane's section on the board.

Gate: none, this is docs only. The PR body lists every measurement that was moved and
where it went.
