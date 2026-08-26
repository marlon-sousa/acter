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

## Amendments made while implementing, 2026-08-26

Three entries landed between this spec's rewrite on 2026-08-24 and its implementation —
B5.4 (the catalogue), A9 (the window says what it is connected to) and B7 (the actions) —
and the reader found two defects in the first build that changed the design rather than
only the code.

### A. The list is kinds with variants, and the backend groups them

Decision 1 lists "cmd, PowerShell, and in debug builds the scripted sessions. Later: WSL",
with the panel holding WSL's distributions. B7 shipped `connectable()` **flat**, one row per
distribution, because it had no panel to put them in. It groups now: `Connectable` gains
`variants`, and a WSL row carries the installed distributions rather than becoming several
rows.

The grouping is the backend's rather than the frontend's, which is decision 3 applied
literally: the frontend knows what a kind *looks like*, the backend knows which things
exist. A frontend that grouped would have had to recognise a distribution by its id and
re-derive the label the backend had already written, which is two places deciding one set of
words.

The variants are named without their kind — "Ubuntu", not "WSL: Ubuntu" — because the row
above has already said it. What a *window title* says is still the full name, because a title
has no row above it to lean on.

### B. A modal dialog is inert to the document's live region

**Found by NVDA, and it disabled decision 2 completely.** `showModal` puts the dialog in the
top layer and makes the rest of the document inert, so the announcer's live region at the end
of `<body>` changes where nothing is listening. Measured on 2026-08-26: arrowing the kinds
announced each one and said *nothing at all* about the panel — precisely the silent-change
trap decision 2 exists to answer.

So a dialog that wants to be heard carries its own live region marked `data-live-region`, and
`AnnouncerDom` drains there while it is open. Found by attribute rather than by name, so the
announcer knows that dialogs exist and nothing about which ones.

**And the announcement stopped repeating the kind.** With the region working, the first
wording — "WSL, 2 distributions" — was heard on top of the listbox's own "WSL 4 of 8", so a
listener heard the kind twice for one arrow press. What the announcement adds is the half no
widget can say for itself, so it says only that: "2 distributions". The same measurement made
the dialog silent on *opening* onto an empty panel, because a reader reads a dialog as it
opens, including a live region inside it that already has text.

### C. Tab escapes a modal dialog, and now there is one implementation of not escaping

Also found by NVDA on 2026-08-26: Tab past Cancel announced "dialog Connect" and left the
reader on the dialog element rather than cycling back to the list. This is the same defect
A7 measured in the About dialog on 2026-08-24 and fixed there with a private method — so the
fix moved into `adapters/dialog_tab.ts` and both dialogs call it. Two copies of a focus rule
are two things that can drift, and the one that drifts is the one nobody is testing that
week.

### D. The panel's variants are a combo box, not a second listbox

Decision 1 says the panel holds "the distributions installed on this machine" without saying
what shape they take. A second listbox would be a second widget with its own arrow handling
and its own browse/focus-mode problem; a `<select>` is something a listener opens with
`Alt+Down` and arrows from any mode, and that gesture is in the platform's own accessibility
contract. Measured working: "Distribution combo box collapsed Ubuntu".

### E. Connect joins the Acter menu, because there is no File menu

Decision 1 says "File → Connect". A7 built one menu named **Acter** holding Exit, and a
second named About. Connect goes into the Acter menu, above Exit — above, because it is what
that menu is now mostly opened for, and because the item that ends the application should not
be what an accidental Enter lands on.

### F. `connectTo` answers whether it worked

Decision 4 needs the dialog to close on success and stay open on failure, so B7's
`AppController.connectTo` returns a boolean instead of swallowing the outcome. The sentence
is still announced by the controller, because the words are the backend's and every other
announced string in the frontend is pinned in one module.

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

- [x] File → Connect opens the dialog; Escape and Cancel close it and leave focus in the
      edit field.
- [x] The kinds come from `connectable()` and are asked for fresh each time it opens.
- [x] Changing the kind changes the panel and announces what it now holds.
- [x] Connecting to cmd and to PowerShell replaces the session, and the listener is told
      which far end they are on.
- [x] A connection that fails announces why, keeps the dialog open, and leaves the running
      session untouched — asserted against a kind that cannot start.
- [x] vitest over the dialog's behaviour, and an E2E spec driving the whole path.
- [x] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests, vitest and the
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
