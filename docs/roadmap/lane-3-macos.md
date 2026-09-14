# Lane 3: macOS — history

Entry bodies for this lane. The board is [../ROADMAP.md](../ROADMAP.md); this file holds
what shipped, what was measured, and the findings behind each entry.

**Why a lane and not four entries in lane 2.** Lane 2 is the domain, and none of this is:
what an operating system offers, who signed a file on it, and where its menus live are four
adapter questions that happen to share a platform. They are ordered strictly among
themselves and block nothing in the other two lanes, which is the definition this board
already uses for a lane.

**What macOS is, as a product decision — Decided.** Two kinds and no more: a shell on this
Mac, and SSH. cmd, PowerShell and WSL are Windows things and are absent from the catalogue
entirely rather than offered as unavailable — `WSL (not available)` on a Mac, with
instructions to install Windows, is the absurdity the catalogue policy was written to avoid.

**What was measured before the lane was opened, 2026-08-31**, on macOS 15.0, rustc 1.98.0,
`x86_64-apple-darwin`. The architecture held: `portable-pty`, `russh`, `alacritty_terminal`
and every domain crate compile for Darwin untouched. Four things did not, and they are most
of entry 28 — **two more appeared once the second was fixed and `acter-app` compiled far
enough to run its own tests**, which is recorded here rather than smoothed over: a survey
that stops at the first thing that will not build has not surveyed anything past it.

- `cargo check --workspace --all-targets` fails once, on an ungated
  `acter_shells::WindowsTrust` import in `crates/acter-transports/tests/real_session.rs`.
- `acter-app` does not compile at all: `generate_context!()` panics with `failed to open
  icon .../icons/icon.png`. The directory holds `icon.ico` and nothing else, and Tauri wants
  a PNG off Windows.
- Three of `acter-core`'s 298 unit tests fail. Two assert on `C:\`-shaped paths in
  `shell_install.rs`; the third, `the_list_asks_the_machine_again_on_every_call`, fails
  because the catalogue is empty off Windows so the list never asks the machine anything.
- `container.rs`'s `profiles_directory()` falls back to `%APPDATA%` and then to `"."`, so a
  macOS Acter would write its `known_hosts` and its explained-shells record into whatever
  directory it happened to be launched from.

And the two that only became visible afterwards:

- **Nine `acter-app` router tests fail** with `connectable not allowed. Plugin not found`. The
  mock invoke names a literal `http://tauri.localhost`, which is *Windows'* local origin; macOS
  serves `tauri://localhost`, so every invoke looks remote and Tauri's ACL refuses it.
- **Four `acter-shells` tests fail and a fifth passes vacuously**, all parsing Windows path
  shapes with `Path`. The vacuous one is the one worth naming: a single unparsed component is
  `Indeterminable` for the wrong reason, so it was green on macOS while asserting nothing.

32. **Done** — M1, Acter runs on macOS, and SSH is what it offers. Spec:
    [m1-acter-runs-on-macos.md](../specs/m1-acter-runs-on-macos.md). The six repairs above, plus
    the catalogue seam and one kind.

    **SSH needs no new adapter and that is the point of putting it first.** `russh` is
    portable, `KnownHosts` reads paths, and `users_known_hosts()` already falls back to
    `HOME`. So the smallest honest macOS build is one that connects somewhere real, which
    makes everything after it judgeable against a green suite and a working window.

    **The catalogue stops being `#[cfg]`-selected** and becomes a policy over the OS name,
    per ARCHITECTURE's platform-divergence rule. That is what repairs the third failing test
    rather than deleting it, and it is what lets a Windows machine run the macOS assertions
    and a Mac run the Windows ones — which neither could do before.

    Also here: a macOS CI job, so the lane cannot regress silently.

33. **Done** — M2, the Terminal row: the shells this Mac has, and who signed them. Spec:
    [m2-the-terminal-row.md](../specs/m2-the-terminal-row.md). One row called Terminal, the
    shells `/etc/shells` names as its variants with the account's own first and marked, and
    macOS's own answer to "who signed this file" in the same PR — because a local row shipped
    without one would raise a security dialog on every connection to a `/bin/zsh` Apple
    signed, and a dialog that always fires is a dialog a listener learns to dismiss.

    **The port was renamed rather than split.** `InstalledShells` is `ThisComputer`: a Mac
    answering `wsl_distributions` with `NotInstalled` states a fact rather than refusing a
    method, `login_shells` is POSIX and Linux will answer it unchanged, and the split these
    questions have was never per platform. Two adapters behind it, two behind `Signatures`,
    and one gated function each in `container.rs`.

    **The measurement obligation was the entry, and it found something.** bash 3.2.57 and zsh
    5.9 ship unchanged — every hostile-rcfile scenario B5.8 and 23.11 measured on Linux was
    re-run against the shells macOS actually ships, and all of them passed. `/bin/sh` did not:
    it is bash 3.2.57 in POSIX mode, so it has readline and honours `\[`, sets no
    `BB_ASH_VERSION`, took the dash branch and cost **sixteen columns** — the exact number
    B9.6 measured for busybox, on the platform where the fix already existed. The branch now
    asks about `$BASH_VERSION` too, which is a fix to shared code that reaches SSH far ends as
    well.

33.1. **Answered, and it was the launch** — VoiceOver was told nothing has keyboard focus
    because the binary was not in a bundle. **Measured 2026-09-02** (VoiceOver, macOS 15.0,
    silent capture, `user` persona), driving M3's checklists against the same debug build
    wrapped in a minimal `.app`: `describe item with keyboard focus` answered correctly
    everywhere it was asked — the help topic's first heading, the Connect dialog's Terminal
    row, the command line, the Results region — where the unbundled build had said "nothing
    has keyboard focus" throughout.

    **The mechanism showed itself in the same run.** Unbundled, the process never becomes
    the *active* application: `go to menu bar` reached **Finder's** menu bar while Acter had
    the window and the keystrokes, and System Events reported acter-app frontmost at the same
    moment. An application macOS does not activate has no keyboard focus to report and no
    menu bar on screen, which is one cause for both symptoms.

    So nothing here is Acter's to fix, and the entry closes into **M4**: what must not happen
    is a later measurement made against an unbundled build and read as a defect. A7's rule
    that focus lands somewhere announced holds on macOS as soon as the application is one.

34. **Done** — M3, the menu bar macOS actually has. Spec:
    [m3-the-menu-bar-macos-has.md](../specs/m3-the-menu-bar-macos-has.md). DESIGN has said since
    A7 that on macOS a menu belongs in the system bar and not in the window; `main.ts`
    honoured the second half of that and nothing honoured the first.

    **"Nothing in its place" was wrong, and the measurement is the entry's first finding.**
    Tauri installs a default macOS menu whenever the builder was given none, so a Mac had a
    menu bar already — with an **empty Help submenu**, with **Connect in no menu at all**, and
    with every window command the platform expects. What this entry replaced was that menu
    rather than an absence.

    **The layout is a value and the platform is an argument**, the shape `offered` has used
    for the connect list since M1: `system_menu(os)` answers with six submenus on macOS and
    with nothing on Windows and Linux, and the composition root attaches a native menu only
    when something was asked for — so the platform where a native menu freezes NVDA for tens
    of seconds cannot acquire one by an edit. Choosing an item Acter owns emits a `MenuAction`
    the frontend switches over exhaustively, into the same actions the document menu bar runs.

    **What it costs is the thing A7 counted as a win**: the in-document menu is drivable by
    WebDriver end to end, and a native one is not. So the E2E suite's menu coverage stays
    Windows-only and this entry's checklist is where the macOS menu is judged.

    **Two facts this left for M4.** The application menu is named by the *process*, not by
    anything Acter can set — proved by renaming the submenu and watching the bar not change —
    so it reads "acter-app" and its Quit says the same until a bundle exists. And macOS
    localises the items it owns, which is why Acter writes only its own items' words: hard
    coding English would have replaced translations on the machine this is built on.

    **And one that makes M4 a dependency rather than a nicety.** The checklist could not be
    run against the unbundled binary at all: macOS never makes such a process the *active*
    application, so `go to menu bar` reached Finder's menu bar while Acter held the window
    and every keystroke. Wrapped in a minimal `.app` the same build announced its own six
    menus immediately. **A menu bar this platform will not display is a menu bar nobody has**,
    so shipping M3's value depends on M4 — and it is what closed 33.1 as well.

    **The checklist itself**, driven with VoiceOver 2026-09-02 (macOS 15.0, silent capture,
    `user` persona): the six menus announced by name; a menu opened in about a quarter of a
    second, against the twenty to sixty-eight seconds A7 measured for a native menu on
    Windows; File announced "Connect… Command k" and opened the dialog on the Terminal row;
    Help announced "Acter Help Command barra" and landed on the topic's first heading; About
    read name, version, copyright and licence; Cmd+K, Cmd+/, fn+F1 and fn+F6 all did what
    they say; and Cmd+C copied the command line into the clipboard, which is what the Edit
    menu is there for.

37. **Done** — a chord the platform owns is never a keystroke for the far end. Spec:
    [37-a-chord-the-platform-owns.md](../specs/37-a-chord-the-platform-owns.md).
    **On macOS every Command chord was typed into the far end.** **Measured 2026-09-03** in a
    real session: `Cmd+K` did not open Connect, it put a `k` on the far end's command line;
    `Cmd+C` did not copy, it put a `c` there. The line then ran as `abcdefkc`.

    **The cause is one missing condition.** `keyboard.ts`'s far-end listener exempts layer 1
    (`Ctrl+Shift`) and nothing else, so a `Cmd+…` chord is read as its letter, sent to the far
    end, and `preventDefault`ed — which also stops the platform's own accelerator, so the
    menu item never fires either. M3's checklist found `Cmd+K`, `Cmd+/` and `Cmd+C` all
    working, and that is not a contradiction: it was run in local-line mode, where this
    listener is not in the path.

    **It is small, and it was the first thing fixed in this lane**: a chord a listener presses
    for their platform's own shortcut must never become input to a remote shell. The entry says
    what the rule is rather than patching one key — which modifiers are the platform's — and
    DESIGN's keystroke map is where that landed.

    **The rule turned out not to need a platform argument at all**, which is smaller than the
    entry expected. The Windows key is the platform's on Windows exactly as Command is on
    macOS, and `event.metaKey` is both; neither is a modifier a terminal has ever carried. So
    one condition is true everywhere: no `os`, no second adapter, no `data-platform`.

    **Both halves matter and only the second is the fix.** Not sending the chord stops the
    stray character; not *preventing* it is what makes the accelerator fire. A version that
    only did the first would have left `Cmd+K` doing nothing at all, which is the same defect
    wearing different clothes.

    **Verified 2026-09-03 in a real session** — a `bash` over SSH to the `docker/ssh` rig, the
    session set up, the far end holding the line — with the row and the focus read out of the
    accessibility API rather than through a screen reader, because the bridge was in use by
    somebody else at the time. With `ech` standing on the far end's line: `Cmd+C` left the row
    at `ech`, `Cmd+/` opened the help topic on its first heading, `Cmd+K` opened the Connect
    dialog on its Terminal row, and leaving that dialog came back to a command line still
    reading `ech`. Nothing ordinary was lost: `Tab` completed it to `echo `, `hi` typed onto
    the end, `Enter` ran it and cleared the row, and `Ctrl+Shift+K` handed the keyboard back.

    **And heard, 2026-09-03**, the same session driven through VoiceOver 15.0 (silent
    capture, `user` persona) so that what a person hears is recorded beside what the tree
    said: with `ech` on the far end's line, `Cmd+C` said **"Copy"** — the Edit menu's own item
    firing, where before it typed a `c` — `Cmd+/` said **"Acter Help"** and landed on a
    level-2 heading in the dialog, and `Cmd+K` said **"Connect…"** and landed in the connect
    list. `Escape` came back to a command line still reading `ech` both times. Ordinary keys
    were untouched: `Tab` said **"o selecionado"** as it completed to `echo `, `Enter` ran it
    and the far end's **"hi"** was spoken, and `Ctrl+Shift+K` answered **"Acter process
    keys."**

    Both halves of that run are the agent's own observation through the screen-readers
    bridge, which is what the checklist rule allows; nothing here needed a sense the bridge
    cannot capture.
