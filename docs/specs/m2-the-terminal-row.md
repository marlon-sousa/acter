# M2 — the Terminal row, the shells this Mac has, and who signed them

Roadmap entry 33, lane 3. It gives macOS a local shell to connect to, and it gives macOS
its own answer to "who signed this file" in the same PR, because shipping the first without
the second would train a listener to dismiss the one dialog in this product that is about
security.

## What is true today

M1 left a macOS build that runs, tests green, and offers exactly one row: SSH. A Mac can
therefore reach somebody else's machine and not its own. `Signatures` falls back to
acter-core's `Unchecked`, which vouches for nothing — correct, and never reached, because
nothing local is startable.

`setup_for` answers `bash`, `zsh`, `sh` and `dash`. Every one of those programs was measured
against a *Linux* shell. macOS ships bash 3.2.57 — 2007, held there by its GPLv2 licence —
and zsh 5.9, and its `/bin/sh` is neither of the two shells the `sh` program was written for.

## What was measured, and where

**On the developer's Mac, 2026-09-01**, macOS 15 (Darwin 24), driving each shell
interactively on a real pseudoconsole fixed at 80 columns, one scenario directory per rc
file, each submitting the setup line **read out of the crate rather than retyped** and then
one command that succeeds and one that exits 7, so a wrong verdict is visible rather than
inferred.

- **bash 3.2.57 passes unchanged.** Four rc files — plain; one that assigns `PROMPT_COMMAND`
  itself; one whose hook runs a command and returns success; one whose hook rebuilds `PS1` —
  each produced the full `C D;0 A B C …` cycle with `D;0` and `D;7` in the right places.
  `true | false` produced `D;1`. A pipeline and a `for` loop each produced exactly one `C`,
  so the `__acter_seen` guard holds against 3.2's `DEBUG` trap. Marked and unmarked, the line
  editor wrapped at the same column.
- **zsh 5.9 passes unchanged.** All five of B5.8's scenarios — plain, a `precmd` function
  that runs a command, one that rebuilds `PROMPT`, a hook already in `precmd_functions`, and
  a `PROMPT_SUBST` prompt — plus the pipeline and loop cases. Same cycle, same verdicts, same
  wrap column marked and unmarked.
- **`/bin/sh` was wrong, and that is this entry's finding.** See decision 7.
- **`0x04` ends a local session** in bash, zsh, `/bin/sh` and dash: the byte was written, the
  shell exited, the child was reaped. **`0x15` discards the pending line** in all four.
- **All seven entries in `/etc/shells` start under `-l`** and reach a prompt.
- **All seven are signed by Apple**, leaf certificate "Software Signing", chain to the Apple
  Root CA.

## Decisions

### 1. `InstalledShells` becomes `ThisComputer`, and stays one port

The board's open question: `wsl_distributions()` is meaningless on a Mac, and a port with a
method every second implementer must refuse has outgrown its name. What had outgrown itself
is the **name**, and the port is renamed rather than split.

**A platform that lacks one of these states a fact rather than refusing one.** There is no
Windows Subsystem for Linux on a Mac, so `NotInstalled` is true there in the plainest sense;
there is no `/etc/shells` on Windows, so an empty list is true there. Neither is ever asked,
because `offered` gives macOS no `Wsl` kind and Windows no `Terminal` kind.

**And the split these questions have is not per platform.** `installs` is asked everywhere;
`login_shells` is POSIX rather than Apple's, so Linux will answer it with the same file and
the same code; only `wsl_distributions` belongs to one operating system. A split by platform
would have put a Unix question in a macOS port and moved it again the day Linux arrived.

This is also what ARCHITECTURE already decided in its platform-divergence rule, which names
`/etc/shells` as the example: an operating system joins by adding an **adapter**, not by
growing a seam.

### 2. Terminal is a kind whose variants are shells, and its row starts the account's own

`ConnectionKind::Terminal`, labelled "Terminal" because that is what a Mac calls it. Its
`program()` is empty, as SSH's is, for a different reason: SSH names nothing because Acter
speaks the protocol itself, and Terminal names nothing because *which* program it is is the
account's own business, read from the machine when the list is built.

A variant is `ProfileId::Install { kind: Terminal, program: "/bin/zsh", provenance: Some("zsh") }`
— no new `ProfileId` variant, and it rides `chosen`'s existing "already resolved" arm, so the
file the panel named is the file that is verified and the file that is started.

**The row's own id is the default variant's**, so Enter with nothing chosen starts what a
Terminal.app window would have started. That is the whole reason the passwd entry is read: a
row that started the first line of `/etc/shells` would start `/bin/bash` for a zsh user and
say nothing about having done so. On the machine this was written on the two differ, which is
how the test was written honestly.

Variants are labelled by the shell's own file name, with `(default)` on the account's own —
in the **name**, for `(not available)`'s reason: nothing about the order of a list is audible.

### 3. `/etc/shells` is the list, and the passwd entry is the default

Two sources, and neither substitutes for the other. `/etc/shells` is the list a Mac itself
uses — what `chpass` accepts and what Terminal.app's preferences offer — so it is the set a
user of this machine already believes they have. Enumerating `/bin` would offer programs
nobody chose to make available.

The account's own shell comes from `getpwuid_r`, **never from `$SHELL`**: that variable is
what started the process that started Acter, inherited through a launcher, editable by any
dotfile, and simply absent for a window opened from the Dock.

**An entry the file names and the machine does not have is not offered**, and one the file
does *not* name but the account logs in to **is** — a shell set with `chsh` from outside the
file is still what a login starts, and a list omitting it would offer everything except the
one thing Enter does.

**An empty list is one situation rather than three**, which is what makes this different from
WSL: a Mac cannot "not have" shells, so an empty answer means the machine is broken in one
way, and `Terminal`'s own instructions name the file to look at.

### 4. A Unix login shell is an adapter of its own, and it is started with `-l`

`UnixShell` rather than `Plain`: a shell chosen from `/etc/shells` differs from an
unrecognised program in three measured ways — it is started as a login shell, it has a setup
when its name is one of the measured four, and `0x04` ends it.

`-l` because that is the session a Mac user already has: `/etc/zprofile`, `~/.zprofile` and
`~/.bash_profile` run, and the `PATH` somebody has spent years arranging is the `PATH` they
get. A shell started without it works and behaves subtly differently from every other
terminal on the machine, which is the kind of difference a listener cannot see and cannot
diagnose.

`0x04` and `0x15` are answered for **every** shell here, including the ones with no setup,
because both are the line discipline's rather than the shell's — which is why `0x15` worked
in dash, that has no line editor at all. A tcsh session that can be ended is strictly better
than one reporting "Acter does not know how".

**No new transport.** `LocalPty` is `portable_pty` and already portable, which is M1's SSH
argument reaching a second kind.

### 5. macOS gets its own `Signatures` adapter, in this entry rather than a later one

`AppleTrust`, over `objc2-security` — Apple's own declarations, the same choice `windows-sys`
is on the Windows side, and B5.7 decision 5's argument that a small low-traffic wrapper crate
is a poor thing to put in the one place where being wrong is a security answer.

**It is declared as a direct dependency** even though Tauri already brings it into the tree:
a crate this product calls is a crate this product depends on, and one that arrives because
somebody else asked for it can leave the same way.

The ladder, because a valid signature is not a trusted one: is the signature intact; is Apple
the anchor; did Apple at least *issue* the certificate; otherwise there is a certificate this
machine has no reason to trust, or no certificate at all.

**`kSecCSCheckAllArchitectures` is a measurement, not a flag copied from a header.** Every
shell macOS ships is a universal binary. Flipping one byte in the middle of a copy of
`/bin/zsh` left it verifying as **signed by Apple** without that flag, because the byte was in
the other architecture's slice. `codesign -v` checks every slice, so a user checking by hand
would have been told what Acter was not.

**`kSecCSNoNetworkAccess` is B5.7 decision 7 on this platform**: the check runs on the
connection, in front of a listener who is waiting.

### 6. `Verdict` stops saying Windows, and `Signer` gains Apple and `Fault` gains AdHoc

Every sentence opened with "Windows trusts this file's signature" and contrasted "rather than
by Microsoft"; a Mac reads them too. They now open with "This computer", and the contrast
clause is gone — it was a comparison a Mac cannot draw, and it said nothing the first half had
not.

`Signer::Apple` joins `Microsoft` for `Microsoft`'s reason: "trusted, and the company that
made this computer signed it" is a different sentence from "trusted, and somebody else did",
and `note()` is silent for both — which is what M2 had to be true of on a machine where every
offered shell is Apple's.

`Fault::AdHoc` is new because macOS makes it ordinary: Apple silicon requires every executable
to carry at least an ad-hoc signature, so a locally built or Homebrew-installed shell has one.
Telling a listener nothing had signed it would be false about the half that is true — the file
has not been altered since it was built. What is missing is who built it.

`Provenance::Windows` becomes `Provenance::System`, which is what it always meant:
`C:\Windows\system32` and `/bin` are the same claim about a file.

### 7. `sh`'s branch asks about `$BASH_VERSION` too, and macOS is what proved two were not enough

**`/bin/sh` on macOS is bash 3.2.57 in POSIX mode.** It has readline, it honours `\[` and
`\]`, and it sets no `BB_ASH_VERSION` — so it took the dash branch and paid busybox's price.
Measured on an 80-column pseudoconsole with a prompt four columns wide: unmarked, the line
editor began a second row at 76 characters; marked, at **60**. Sixteen bytes, sixteen columns,
exactly the number B9.6 measured for busybox, on the platform where the fix already existed
and was not reached.

With `[ -n "$BB_ASH_VERSION" ] || [ -n "$BASH_VERSION" ]`, macOS `sh` goes back to 76 and
draws its prompt byte-for-byte as it does unmarked; dash 0.5.12 sets neither variable, takes
the branch it took before, and still draws no literal brackets.

**What changed is the question the test asks.** It used to mean "is this busybox"; it now
means "does this shell have a line editor that honours the non-printing brackets", and two
shells answer yes for unrelated reasons. **This is not a macOS fix** — it is a fix to a shared
program that macOS found, and it reaches any SSH far end whose `sh` is bash.

### 8. Two adapters per port, and the gate is one function each

`windows_machine` and `unix_machine` behind `ThisComputer`; `windows_signatures` and
`macos_signatures` behind `Signatures`; `machine()` and `signatures()` in `container.rs`
choose. That is ARCHITECTURE's rule taken at its word, and the memory of M1's lesson: both
machine adapters **compile on both platforms**, so `/etc/shells` parsing is asserted on
Windows CI too, with one `#[cfg]` around the passwd lookup rather than around the module. Only
the two signature adapters are gated whole, because each reads an API that exists on one
platform.

Off Windows the machine arm is **Unix rather than macOS**: a Linux build gets a correct answer
from it rather than a stub. What Linux will not get until somebody does the work is a
`Terminal` kind in its catalogue, which is `offered`'s answer and not this one's.

### 9. The frontend calls a Mac's variants shells

`noun()` read the variant's *shape* and answered "edition" for both `Shell` and `Install`.
A Terminal row's variants are `Install`s, so the panel would have said "choose an edition
first" about `/bin/zsh` — naming it after a Windows product a listener has never met. The
kind decides it, which is a fact the switch was already one field away from.

## Files touched

- `crates/acter-core/src/ports/driven/this_computer.rs` (renamed from `installed_shells.rs`):
  the port, plus `login_shells` and the `LoginShell` value.
- `crates/acter-core/src/entities/connection_kind.rs`: `Terminal`.
- `crates/acter-core/src/entities/shell_install.rs`: `Provenance::System`.
- `crates/acter-core/src/entities/signature_verdict.rs`: `Signer::Apple`, `Fault::AdHoc`, and
  the sentences that named one operating system.
- `crates/acter-core/src/policies/catalogue.rs`: macOS offers Terminal, then SSH.
- `crates/acter-core/src/services/connect.rs`: `terminal_row`, the availability arm, the
  `chosen` arm, and `named` taking the label it qualifies.
- `crates/acter-shells/src/unix_machine.rs`, `unix_shell.rs`, `macos_signatures.rs`: new.
- `crates/acter-shells/src/windows_machine.rs`, `windows_signatures.rs`: renamed from
  `installed.rs` and `signature.rs`, type renamed to `WindowsMachine`.
- `crates/acter-shells/src/setup.rs`: decision 7.
- `crates/acter-shells/Cargo.toml`: `libc`, `objc2-security`, `objc2-core-foundation`.
- `crates/acter-app/src/container.rs`: `machine()`, `signatures()`'s macOS arm, and the
  branch that starts a Terminal profile.
- `ui/src/adapters/connect_dialog.ts`, `ui/src/protocol.ts` (generated).

## Definition of done

- A Mac's connect list is Terminal then SSH; the panel holds the shells `/etc/shells` names,
  the account's own first and marked as the default.
- Enter on the row with nothing chosen starts the account's own login shell, as a login shell.
- Connecting to zsh, bash or sh runs that shell's own measured setup line and the session
  reports command boundaries and exit codes; connecting to tcsh, csh or ksh starts, is named,
  and is not experimented on.
- No signature dialog on a connection to a shell Apple signed; an altered or unsigned file
  raises one, and it is never a filter.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets` and the UI suite green on
  macOS and on Windows CI.
- The VoiceOver checklist in the PR body, with each item marked agent-observed or
  human-verified.

## Accessibility checklist (PR body)

One item per check, findings written inline on the unchecked item, naming the VoiceOver
version and the capture mode. The bridge captures speech and braille, not audio, so anything
turning on a sound cue is the human's.

1. The connect list announces "Terminal" as a row, and the list is two rows long.
2. Arrowing to Terminal and tabbing to the panel announces it as a list of shells, with a
   count.
3. The account's own shell is announced first and its name carries "default".
4. Choosing a shell and pressing Connect starts it, and the connection sentence names the
   shell that was started.
5. Connecting to a shell Apple signed says nothing about signatures.
6. A session in zsh announces command boundaries and a failed command's exit code.
7. A session in tcsh announces that Acter cannot tell how commands went, rather than going
   silent.
8. Nothing in the flow depends on a sound cue rather than speech. (Human.)
