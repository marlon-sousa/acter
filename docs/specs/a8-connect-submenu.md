# A8 — the Connect submenu

Roadmap entry 13, lane 1. Agreed in conversation 2026-08-23. **Depends on 25 (B7)** for the
actions it triggers, and on A7 for the menu it hangs from — the one cross-lane dependency in
this group.

The entry that turns "the far end is whatever the environment said at launch" into "the user
picks a shell and gets it".

**This was a modal dialog until the profile conversation replaced it**, and the reasoning is
worth keeping: a submenu needs no focus trap, no modal semantics, no second surface and no
Escape rule, and arrowing through a list of items — with first-letter navigation — is what
menu navigation is already best at for a screen reader user. A dialog earns its place when
connecting needs more than a choice. It does not.

## Decisions

### 1. Connect is a submenu of the Acter menu, one item per connectable thing

```
Acter
  Connect  >  cmd
              PowerShell
              PowerShell 7
              WSL: Ubuntu
              WSL: docker-desktop
              Scripted: builtin            (debug builds only)
  Exit
```

Flat, per DESIGN: "WSL: Ubuntu" is an item, not a WSL item that opens a third level. Depth
costs a screen reader user more than length does.

The items are exactly what `connectable()` returned, in that order, with their labels
unchanged. The menu invents nothing, sorts nothing and hides nothing — a menu that reordered
or filtered the backend's answer would be a second place where the list is decided.

### 2. The submenu is rebuilt each time the menu bar is opened

`connectable()` is asked again, because a distribution installed while Acter is running must
appear without a restart. Rebuilding a native submenu is cheap and the list is small.

**If the rebuild is not possible on menu-open** — `muda` may not expose a hook at the moment
the bar is activated — the fallback is rebuilding after every successful connect and at
startup, and the limitation is written into this spec rather than left as a surprise. This
is measured during implementation, not assumed either way.

### 3. Choosing an item runs three steps, in this order

1. `use_profile(id)` — the backend starts the new session, drops the old one, and answers
   with its id and label.
2. The frontend clears the results buffer, so nothing from the previous shell is left under
   a heading belonging to a session that no longer exists.
3. `attach_session(new_id)`, then focus to the edit field, then one announcement naming what
   the user is now connected to.

**The announcement is not decoration.** After connecting, the window looks exactly as it did
and answers differently; that sentence is the only thing telling a listener which shell they
are typing into. It is a pinned string in `controllers/app.ts` beside every other one.

The buffer is cleared rather than kept, because a buffer holding two shells' output has
headings that no longer mean what they say, and F6 and heading navigation would walk a
listener through a session that is gone.

### 4. A failure is announced, and the session that was running is still there

The backend's sentence is announced; the previous session stays attached and working,
because B7 guarantees it was never dropped. Focus returns to the edit field. Nothing else
changes, and the user can open the menu and pick something else.

### 5. Menu items reach the frontend as events, and the frontend does the work

A7's decision, applied: `on_menu_event` emits one event per item; the frontend listens and
runs the three steps. Rust opening dialogs or clearing buffers would put user-interface
behaviour on the wrong side of the seam and outside every suite we have.

**What is testable here and what is not**, stated plainly: the three-step flow, its failure
path and its announcements are covered in vitest by driving the frontend's menu-event
handler directly, and the actions behind them are covered in Rust by B7. What no suite
reaches is the item existing in a native submenu and firing when chosen — that is the NVDA
checklist below, and it is the same trade A7 made.

## Files touched

- `crates/acter-app/src/menu.rs` — the Connect submenu, built from `connectable()`, and its
  item ids.
- `crates/acter-app/src/container.rs` — rebuilding the submenu, and routing its events.
- `ui/src/controllers/app.ts` — the connect flow, its pinned strings, the menu-event
  listener.
- `ui/src/ports/backend_api.ts`, `ui/src/routers/tauri.ts` — the two commands.
- `ui/test/controllers/app.test.ts` — the flow, the failure path, the announcements.

## Definition of done

- [ ] The submenu contains exactly what `connectable()` returned, in order, labels
      unchanged.
- [ ] Choosing an item runs the three steps in order, with the announcement last.
- [ ] A failed connect announces the backend's sentence and leaves the previous session
      attached and working.
- [ ] Debug builds list the scripted sessions; a release build does not.
- [ ] The rebuild rule is whatever was measured to be possible, and this spec says which.
- [ ] vitest covers the flow and its failure path; router tests cover both commands.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests, vitest and E2E
      clean.

## Accessibility checklist for the PR body

Agent-observable through the screen-readers bridge, `user` persona, recorded inline with the
NVDA version and capture mode:

- [ ] Connect is announced as a submenu, and opening it announces the first item and how
      many there are.
- [ ] Arrow keys move through the items, each announced with its label; first-letter
      navigation reaches an item by its first character.
- [ ] Choosing an item announces which far end the session is now on, and the edit field has
      focus afterwards.
- [ ] A command submitted immediately after connecting runs in the new shell and is heard.
- [ ] Escape leaves the menu with nothing announced beyond focus returning, and no session
      change.
- [ ] Connecting from an unconnected window works and is announced the same way as
      connecting from a running one.
- [ ] Choosing a WSL distribution that has been uninstalled since the menu was built
      announces why it failed and leaves the previous session answering. **Human-verified**:
      it needs a distribution removed between two moments, which the bridge cannot do.
