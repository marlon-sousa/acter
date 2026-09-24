# 55 — The first Windows release is the 0.1 beta.

Lane: lane-6-release. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**The first Windows release is the 0.1 beta.** Spec: none yet → specify first.

Recorded on 2026-09-23 from the planning conversation. The user wants Acter 0.1 released as a
beta for Windows. It is **the last step**: it comes after lane 5's visual entries are fixed
and, possibly, after the entries found while stripping comments in lane 4 (42 to 51).

What exists, read from the repository on 2026-09-23:

- `.github/workflows/release.yml` (spec 26, decision 21) builds a Windows installer (Tauri's
  NSIS target, `installMode: currentUser`) and a portable zip, the zip compiled with the
  `portable` feature. It runs when a tag shaped `windows-vX.Y.Z` is pushed, and it refuses a
  tag whose commit is not on main.
- `gh run list --workflow release.yml` lists no runs, and there are no tags or releases. The
  workflow has never run.
- The README's "Installing" section names `acter_x.y.z_x64-setup.exe` on the releases page,
  and it says there is no signing certificate yet, so SmartScreen reports an unknown
  publisher.
- `tauri.conf.json` and `ui/package.json` both say version `0.1.0`. The workflow takes the
  version from the tag.

Unknown: whether the tag pattern and the version stamping accept a pre-release version such
as `0.1.0-beta.1`, and what a beta tester is told about what works and what does not.
