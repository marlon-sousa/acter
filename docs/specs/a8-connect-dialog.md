# A8 — the Connect dialog

Roadmap entry 13, lane 1. Agreed in conversation 2026-08-23. **Depends on 25 (B7)** for its
two commands, and on A7 for the menu it hangs from — the one cross-lane dependency in this
group.

The dialog that turns "the far end is whatever `ACTER_SHELL` said at launch" into "the user
chooses a shell and gets it".

## Decisions

### 1. A flat list of radio buttons, not a tree and not a wizard

The dialog holds one group: **Connect to**, with one radio per connectable thing, in the
order the backend returned them.

```
Connect to
  ( ) cmd
  ( ) PowerShell
  ( ) PowerShell 7
  ( ) WSL: Ubuntu
  ( ) WSL: docker-desktop
  [Connect]  [Cancel]
```

**Flat, per DESIGN**: "WSL: Ubuntu" is an entry rather than a WSL entry that reveals a second
control. A conditional control that appears only for one option is a change in the shape of
the dialog under a user who cannot see it change, and it makes the number of things to
arrow through depend on where you are. A radio group is the one control non-visual
navigation is unambiguously good at: NVDA announces the group, the item, and its position
in the group, arrow keys move within it, and first-letter navigation works.

**Not a `<select>`**, whose collapsed state hides how many choices exist, and **not a list of
buttons**, which would connect on the first Enter with no chance to review the choice.

### 2. The list is fetched every time the dialog opens

`connectable()` on open, never cached. A distribution installed while Acter is running must
appear the next time the dialog is opened, and a cached list would be wrong with nothing
telling the user so.

While it is being fetched the dialog is already open and says so, because the call runs
`wsl.exe` and can take a moment on a cold machine.

### 3. Connecting is three steps, in this order, and the order is the accessible part

1. `connect(id)` — the backend builds the new session, drops the old one, and answers with
   its id and label.
2. The frontend clears the results buffer, so nothing from the previous shell is left under
   a heading belonging to another session.
3. `attach_session(new_id)`, then focus to the edit field, then one announcement naming what
   the user is now connected to.

**The announcement is not optional and it is not decoration.** After a connect, the window
looks exactly as it did and answers differently; the sentence is the only thing that tells a
listener which shell they are typing into. It is a pinned string in `controllers/app.ts`
beside every other pinned string.

The buffer is cleared rather than kept, because a buffer that mixes two shells' output has
headings that no longer mean what they say, and F6 and heading navigation would walk a
listener through a session that no longer exists.

### 4. A failure keeps the dialog open and says why

The backend's sentence is announced and the dialog stays open with the selection intact, so
the user can pick something else or cancel. The session they had is still running and still
attached — B7 guarantees it was never dropped — so cancelling after a failure returns them
to a working session rather than to nothing.

### 5. Cancel changes nothing at all

Escape and the Cancel button both close the dialog, focus returns to the edit field, and no
command is sent. Nothing is announced beyond the focus change, because nothing happened.

### 6. Connect… joins the Acter menu here

A7 deliberately shipped the menu without it. This entry adds the item, its id, and its
handler, which is a three-line change to A7's pure menu definition plus its test.

## Files touched

- `crates/acter-app/src/menu.rs` — the Connect… item and its id.
- `ui/src/views/main_window.html` — the dialog's static skeleton: the `<dialog>`, the
  `<fieldset>` and its legend, the two buttons.
- `ui/src/adapters/connect_dialog.ts` — open, render the options, read the selection, focus
  and Escape (role: adapter).
- `ui/src/controllers/app.ts` — the connect flow, its pinned strings, and the menu-event
  listener.
- `ui/src/ports/backend_api.ts`, `ui/src/routers/tauri.ts` — the two commands.
- `ui/test/adapters/connect_dialog.test.ts`, `ui/test/controllers/app.test.ts`, `e2e/` —
  tests.

## Definition of done

- [ ] The dialog lists exactly what the backend returned, in that order, one radio each.
- [ ] Connecting runs the three steps in order: connect, clear, attach — with the
      announcement last.
- [ ] A failed connect keeps the dialog open, announces the backend's sentence, and leaves
      the previous session attached and working.
- [ ] Cancel and Escape send nothing and return focus to the edit field.
- [ ] Debug builds list the scripted sessions; a release build does not.
- [ ] vitest covers the adapter and the controller flow including the failure path; the E2E
      suite drives the dialog in the real WebView2; router tests cover both commands.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests, vitest and E2E
      clean.

## Accessibility checklist for the PR body

Agent-observable through the screen-readers bridge, `user` persona, recorded inline with the
NVDA version and capture mode:

- [ ] Connect… is reachable in the Acter menu and announced.
- [ ] The dialog is announced as a dialog with its name, and the radio group is announced
      with its legend.
- [ ] Arrow keys move through the options; each is announced with its label and its position
      in the group.
- [ ] Tab reaches Connect and Cancel and does not leave the dialog.
- [ ] Connecting announces which far end the session is now on, and the edit field has focus
      afterwards.
- [ ] A command submitted immediately after connecting runs in the new shell and is heard.
- [ ] Escape closes the dialog with nothing announced beyond the edit field regaining focus.
- [ ] Connecting to a WSL distribution that has been uninstalled since the list was built
      announces why it failed, and the previous session still answers. **Human-verified**:
      it needs a distribution uninstalled between two moments, which the bridge cannot do.
