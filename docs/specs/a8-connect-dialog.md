# A8 — the Connect dialog

Roadmap entry 13, lane 1. **Rewritten 2026-08-24**, agreed in conversation, replacing the
submenu this entry was between 2026-08-23 and then. **Depends on 25 (B7)** for the actions
it triggers, and on A7 for the menu it hangs from — the one cross-lane dependency in this
group.

The entry that turns "the far end is whatever the environment said at launch" into "the user
picks a shell and gets it".

## Why this stopped being a submenu

It was a dialog, then a submenu, and it is a dialog again. That is worth recording properly,
because each move was made for a reason and the last one retires the reason for the middle
one.

The submenu argument was navigational, and as far as it goes it is still true: a submenu
needs no focus trap, no modal semantics, no second surface and no Escape rule, and arrowing
a list of items with first-letter navigation is what menu navigation is best at for a screen
reader user. Depth costs more than length.

**What that argument assumed is that connecting is a choice.** For cmd, PowerShell and a WSL
distribution it is. For SSH it is not: a host, a port, a user and a key are a *form*, and no
submenu holds one. The alternatives were a submenu for the easy kinds beside a dialog for
SSH — two ways to do one thing, which is worse for everybody and worst for someone learning
the application by ear — or one surface that can carry both. This entry is the second.

**And the testability half of the old trade reversed too.** The submenu decision accepted
that no suite in this project could drive it. A7 moved the menu bar into the document after
measuring that a native one freezes NVDA for tens of seconds, so the dialog is ordinary DOM:
vitest drives its behaviour and WebDriver drives the whole path. What used to be a cost is
now a gain.

## Decisions

### 1. One dialog, two parts: a kind, then what that kind needs

File → Connect opens a modal `<dialog>` holding:

- **A list of connection kinds** — a listbox, arrowed like any other. Today: cmd,
  PowerShell, and in debug builds the scripted sessions. Later: WSL, SSH, and the user's
  saved connections.
- **A panel below it** carrying whatever that kind needs, and nothing when the kind needs
  nothing. cmd and PowerShell: nothing, so the dialog is a list and a Connect button. WSL:
  the distributions installed on this machine. SSH: host, port, user. Saved connections:
  the list of them.

### 2. The panel announces itself when the kind changes

This is the whole of the old decision's objection, and it is answered rather than accepted.
A conditional second control that appears silently is the classic non-visual trap: the
listener arrows the kinds, the screen changes behind them, and nothing says so.

So: **changing the kind announces what the panel now holds** — "no options" for cmd, "three
distributions" for WSL, "four fields" for SSH — through the announcer this product already
has. Tab from the kind list lands in the panel, and Tab from the panel reaches Connect.
Arrowing the kinds never moves focus into the panel by itself, because a list you cannot
arrow through without leaving it is not a list.

### 3. Kinds are a closed set the frontend can render; variants are the backend's

The division matters, and it is the one thing in this entry that is not obvious.

**The frontend knows the kinds**, because rendering them is what it is for: it cannot draw a
form for a kind nobody told it about, and a backend that described its own controls would be
a user interface written in Rust and reachable by no test. Adding a kind is a frontend
change plus a backend one, deliberately.

**The backend knows the variants**, and the frontend hardcodes none of them: which
PowerShell editions are installed, which distributions exist, which connections the user has
saved. That is `connectable()` (B7, decision 6), asked fresh every time the dialog opens, so
a distribution installed while Acter is running appears without a restart.

### 4. Connecting is the same three steps the submenu would have run

1. `use_profile(id)` — the backend starts the new session, drops the old one, and answers
   whether it worked. B7 decision 5 already requires the shape this dialog depends on: the
   new session is built *before* the old one is dropped, a failure is a speakable sentence,
   and the session that was running is still running.
2. On success the dialog closes, focus returns to the edit field, and the listener is told
   which far end they are on now.
3. On failure **the dialog stays open**, the sentence is announced, and focus stays where
   the user can fix what they typed. This is where a dialog beats a submenu a second time:
   a submenu that failed had nowhere to put the user back.

### 5. Saving comes after the probe, and it is what makes profiles real

A connection that came up is offered for saving; one that did not is not. That is the whole
rule, and it is what turns profiles from files a user hand-edits into something the
application creates — DESIGN's profiles section says the in-app flow arrives "when one earns
its place", and this is it earning it.

**It also settles the ordering**: the JSON profile store (B8) is built after WSL lands,
because a save flow with only cmd and PowerShell behind it saves nothing a user could not
retype in a second. Saved connections then appear as their own kind in the list of 1.

**Not in this entry**: what a saved connection stores beyond its kind and its variant —
starting directory, auto-read threshold, the rest of the profile — is B8's, and this dialog
gains a field for each of them the day they exist rather than guessing now.

### 6. What is deliberately absent

- **No "something is still running" confirmation**, per DESIGN: the answer it would need is
  stuck at "yes" for the whole life of a real session until roadmap 22.8 lands, and a
  confirmation that fires every time teaches the user to dismiss it.
- **No connection editing.** Saved connections can be chosen and saved, not renamed or
  deleted, until somebody asks for it.

## Files touched

- `ui/src/views/main_window.html` — the dialog's static skeleton: the kinds listbox, the
  panel region, Connect and Cancel.
- `ui/src/adapters/connect_dialog.ts` — the kind/panel behaviour and the announcement on
  change (new; role: adapter).
- `ui/src/adapters/menu_bar.ts` — Connect joins the File menu.
- `ui/src/ports/app_shell.ts` — `connectable()` and `use_profile()` reach the frontend
  beside `about` and `platform`.
- `ui/src/routers/tauri.ts` — their invoke wrappers.
- `crates/acter-app/src/routers/` — the two commands, which are B7's to build.
- `ui/test/adapters/connect_dialog.test.ts` and an `e2e/` spec — its tests.

## Definition of done

- [ ] File → Connect opens the dialog; Escape and Cancel close it and leave focus in the
      edit field.
- [ ] The kinds come from `connectable()` and are asked for fresh each time it opens.
- [ ] Changing the kind changes the panel and announces what it now holds.
- [ ] Connecting to cmd and to PowerShell replaces the session, and the listener is told
      which far end they are on.
- [ ] A connection that fails announces why, keeps the dialog open, and leaves the running
      session untouched — asserted against a kind that cannot start.
- [ ] vitest over the dialog's behaviour, and an E2E spec driving the whole path.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests, vitest and the
      E2E suite all clean.

## Accessibility checklist for the PR body

Agent-observable through the screen-readers bridge, `user` persona, recorded inline with the
NVDA version and capture mode:

- [ ] File → Connect opens a dialog, announced as one, with its name.
- [ ] The kinds are a list that arrows, each announced with its name and position.
- [ ] Changing the kind announces what the panel now holds, without moving focus.
- [ ] Tab reaches the panel and then Connect; Shift+Tab comes back; nothing escapes the
      dialog.
- [ ] Connecting announces the far end the user is now on, and focus is in the edit field.
- [ ] A failed connection is announced as a sentence a listener can act on, and the dialog
      is still there.
