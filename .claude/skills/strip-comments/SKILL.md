---
name: strip-comments
description: Work one roadmap lane 4 PR (C1 to C11) that strips comments from Acter's Rust and TypeScript under the "comments say what the code cannot" rule. Use when asked to do a C-numbered PR, to strip or reduce comments in a file or crate, or to bring a file under the comment ratio.
---

# Stripping comments for one lane 4 PR

The contract is [docs/specs/c-comments-say-what-the-code-cannot.md](../../../docs/specs/c-comments-say-what-the-code-cannot.md).
Read its rubric and its entry for the PR you are doing before touching a file. This
skill is the procedure; the spec is the judgement.

## Before starting

1. Find the PR's file list in the spec. Touch those files and no others.
2. Run `python scripts/comment_ratio.py <files>` and keep the total line for the PR body.
3. Branch off main.

## For each file

1. Read it top to bottom. Every comment paragraph gets exactly one bucket from the
   rubric: N1 to N7 delete, K1 to K6 keep.
2. Delete N paragraphs whole. Compress K paragraphs to one plain sentence that states
   the fact, naming the program and version on a measured fact, never a date. A kept fact appears once in the
   repository; a second place says "see" with a path.
3. A comment that contradicts the code is deleted if it is N and corrected if it is K.
   Never change the code to match a comment. Put the contradiction in the PR body under
   "found while stripping".
4. Change no line that is not a comment. The one exception is renaming a test whose
   narration you deleted so that the name carries the intent.
5. Run the script on the file again. At or under 0.20 comment lines per code line, or
   every remaining comment is K2 or K3 and the PR body says so for that file.

## Before opening the PR

1. Run the gate named in the spec for this PR. A red gate means a non-comment line
   moved; find it and revert it. C2 also regenerates `ui/src/protocol.ts` and commits it.
2. Run the script on the PR's files again for the "after" total.
3. Flip the PR's line on the board in `docs/ROADMAP.md` to Done, in the lane list and
   under "What is next". Write nothing else there: the PR body is the record.

## PR body

- The script's total line before and after. The code count must be identical; only the
  comment count moves.
- "No non-comment line changed", or the list of test renames.
- "Found while stripping": each comment that contradicted the code, with file and line,
  or "none".
- Any file left above 0.20 and why.
