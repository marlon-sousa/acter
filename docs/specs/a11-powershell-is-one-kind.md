# A11 — PowerShell is one kind, and its editions are its variants

Roadmap entry 13.6, lane 1. Agreed in conversation 2026-08-26, while A10 was being tested.
Depends on **A8** for the panel the editions live in and on **B7** for the list they come
from.

## What the user asked for, and why it is right

> *"Can you create powershell as a single division, and consider the different versions as
> connection properties, like you did with wsl?"*

The connect list had four kinds: Command Prompt, Windows PowerShell, PowerShell 7, WSL. Two
of those four differ by one word, and a listener arrowing the list has to hold both in mind
to tell them apart — while WSL, which comes in as many flavours as the machine has
distributions, is one row with a panel.

The asymmetry was an accident of history rather than a decision. B5.4 made the editions two
kinds because there was nowhere else to put them; A8 built the panel that is where they
belong.

## Decisions

### 1. Three kinds, and editions are variants

`ConnectionKind::PowerShell` joins the catalogue and the two editions leave it. The list is
now **Command Prompt, PowerShell, WSL** — three rows however many editions and distributions
this machine happens to have, which is the number a listener can learn.

`WindowsPowerShell` and `PowerShellSeven` stay in `ConnectionKind`: they are still things a
user can choose, still have their own labels and their own instructions, and are now reached
through the panel. What changed is only that `ON_THIS_PLATFORM` no longer lists them.

### 2. Editions are *known*; distributions are *discovered*

This is the one real difference between the two kinds, and it decides everything below.

Which PowerShell editions exist is the same answer on every machine in the world, so
`ConnectionKind::editions()` names them. Which are *installed* is the machine's answer, asked
through `InstalledShells::is_available`. Which Linux distributions exist at all is the
machine's answer twice over, so WSL's variants can only be enumerated by running `wsl.exe`.

### 3. A variant can be missing while its kind is not, and it still says what to do

**B5.4's argument, one level down.** A machine with Windows PowerShell and no PowerShell 7
has the kind and one of its two editions. Listing only what is installed would teach that
listener that Acter does not support the edition they have read about — which is exactly the
lesson B5.4 refused to teach about WSL.

So `Variant` carries `available` and `instructions`, a missing edition keeps its place in the
panel with `(not available)` in its **name**, and choosing it shows what to type.

A WSL distribution is always available, and that is not an oversight: one that is not
installed has no name to enumerate.

### 4. The row starts something even if the panel is never opened

The PowerShell row's `id` is the first edition that can actually be started, so choosing the
row and pressing Connect starts Windows PowerShell rather than failing. The row is available
when *any* edition is — a machine with PowerShell 7 and no Windows PowerShell still has
PowerShell.

When neither edition is installed the row is unavailable, sorts to the end, and carries a
sentence of its own: a Windows install with no PowerShell at all is broken rather than
unsupported, and saying so is more use than an empty panel.

### 5. The panel names itself after what it holds, and never changes in silence

The summary and the control's label come from the variants' own shape: "2 editions" and
`Edition` for PowerShell, "3 distributions" and `Distribution` for WSL. That is the
frontend's knowledge by A8's decision 3 — the backend says which things exist, this side says
what they are called on screen.

**Choosing an unavailable edition announces itself and shows what to do**, for the reason
A8's decision 2 exists: a panel that changes under a listener without saying so is the
classic non-visual trap, and it applies to the inside of a panel as much as to its arrival.

## Files touched

- `crates/acter-core/src/entities/connection_kind.rs` — the `PowerShell` kind, `editions()`,
  and its own instructions.
- `crates/acter-core/src/policies/catalogue.rs` — three kinds rather than four.
- `crates/acter-core/src/entities/protocol_commands.rs` — `Variant` gains `available` and
  `instructions`.
- `crates/acter-core/src/services/connect.rs` — `powershell_row`, and availability that
  means "any edition".
- `ui/src/adapters/connect_dialog.ts` — the noun, the control's label, and an unavailable
  variant's instructions.

## Definition of done

- [x] The list is Command Prompt, PowerShell, WSL, and the scripted sessions in a debug
      build.
- [x] PowerShell's panel lists both editions, the missing one included, saying so in its
      name and carrying what to type.
- [x] Choosing the row without opening the panel starts an edition that works.
- [x] A machine with neither edition still lists PowerShell, unavailable and last.
- [x] Choosing an unavailable edition is announced, shows its instructions, and is refused by
      the backend with the same words.
- [x] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests, vitest and the
      E2E suite all clean.

## Accessibility checklist for the PR body

- [ ] The kinds list reads as three rows, and PowerShell is one of them.
- [ ] Arrowing onto PowerShell announces "2 editions", and Tab reaches a combo box called
      Edition.
- [ ] Choosing the missing edition is announced, and its instructions are reachable and read.
- [ ] Connecting from the panel starts the edition chosen, and the window names it.
