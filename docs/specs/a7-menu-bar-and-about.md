# A7 — the menu bar and About

Roadmap entry 12, lane 1. Agreed in conversation 2026-08-23.

Acter has no application surface. There is a window, an edit field and a results region, and
the far end is chosen by an environment variable read at launch. This entry gives it a menu
bar — and the entry after it puts Connect in that menu.

## The measurement that comes first, because it can invalidate the design

**A native window menu is only a menu if a keyboard user can open it.** Microsoft documents
that application accelerators such as Alt+X do not fire while focus is *inside* the
WebView2 control, and do fire when focus is in a native control (WebView2Feedback #1703).
`muda` documents that accelerators need `TranslateAcceleratorW` in the message loop. Acter's
focus is inside the webview essentially always, because the edit field is where the user
lives.

So before anything is built, three questions are answered against a real window with a real
NVDA:

1. **Does Alt reach the menu bar** when focus is in the edit field?
2. **Does NVDA announce it** as a menu bar, with its items and their submenus, and do arrow
   keys and Enter behave the way they do in any other Windows application?
3. **If Alt does not reach it, does `WM_SYSCOMMAND` with `SC_KEYMENU`**, posted to the window
   from Rust in response to a key the webview *does* receive, activate the menu bar?

Written up as this spec's opening section before any implementation, the way B4.3 and B4.6
were, and reported plainly whichever way it goes.

**What each outcome means.** If 1 and 2 hold, the design below stands as written. If 1 fails
and 3 works, the menu gains an Acter binding to open it — `Ctrl+Shift+M`, which fits
DESIGN's own layer-1 rule that Ctrl+Shift means the user is talking to Acter — and that
binding becomes a Decided part of the keystroke map rather than a workaround. If 1 and 3
both fail, **DESIGN's native-menu decision is reopened with the measurement in hand**, and
this entry does not proceed on the strength of a preference.

## What can and cannot be tested

`MockRuntime` does not execute native webview libraries, and nothing in Tauri documents
driving a menu or `on_menu_event` under it. WebDriver drives the webview only. So **no
suite in this project can open this menu**, and pretending otherwise with a test that
asserts a builder did not panic would be worse than admitting it.

The design answers it by splitting the menu in two:

- **A pure function returning the menu as data** — a small tree of ids, labels and nesting,
  with no Tauri types in it. Plain `cargo test` asserts the structure, the labels, the ids
  the event handler matches on, and that every id an item can emit is one the handler
  handles. That last one is the assertion that actually catches a bug.
- **A thin construction layer** turning that data into Tauri's `MenuBuilder` calls, kept
  boring enough that reading it is sufficient review, and verified by the NVDA pass.

Everything the menu *opens* is HTML, and HTML is testable by both suites — which is another
reason the dialogs are not native.

## Decisions

### 1. Two menus, and About holds one item

- **Acter** — Exit. (Connect… is added by A8, the day it works. A menu item that opens
  nothing is worse than a menu that does not offer it yet.)
- **About** — About Acter.

About is a menu containing one item rather than a top-level item that fires, because a menu
bar entry that acts instead of opening is a surprise to somebody arrowing along the bar, and
every other Windows application they know behaves the other way.

### 2. Exit ends the session as well as the window

Closing the application must take the shell with it. `LocalPty::drop` already kills the
shell it spawned, so what this decision really requires is that the session is *dropped* on
exit rather than the process being torn down around it. A shell that outlives the window
that opened it is a leak the user cannot see — B4's own words.

Two caveats recorded from 22.6, so nobody reopens them as bugs: a far end the user proxied
into (a container, a typed `ssh`) is not in Acter's process tree and does not go away, and
that is exactly what a real `cmd.exe` window does too.

### 3. The About dialog is an HTML modal, and its content comes from the build

A `<dialog>` in the main window: product name, version, copyright, licence. The values are
read from the build — `CARGO_PKG_VERSION` and the licence field the workspace already
carries — and reach the frontend through one router command rather than being typed into
HTML where they would be wrong at the next release.

The copyright line is the one in `LICENSE`: **© 2026 Marlon Brandão de Sousa**, MIT.

Dialog behaviour, all of which is testable and all of which is a domain requirement here:

- It is a modal `<dialog>` with an accessible name, so NVDA announces a dialog and reads it.
- Focus moves into the dialog when it opens and is trapped there while it is open.
- Escape closes it, and so does its close button.
- **On close, focus goes to the edit field.** It cannot return to whatever opened it,
  because what opened it was a native menu outside the document — so the rule is stated
  rather than left to the browser.

### 4. The menu talks to the frontend by event, and the frontend owns what happens

`on_menu_event` in the composition root emits one event per menu action to the webview; the
frontend listens and acts. The alternative — Rust opening dialogs — would put user-interface
behaviour on the wrong side of the seam and outside every test suite we have.

## Files touched

- `crates/acter-app/src/menu.rs` — the pure menu definition and its ids (new; role:
  entity/value for the definition, adapter for the construction).
- `crates/acter-app/src/container.rs` — building the menu, attaching it to the window,
  routing `on_menu_event` to webview events.
- `crates/acter-app/src/routers/about.rs` — the command answering name, version, copyright
  and licence.
- `ui/src/views/main_window.html` — the About dialog's static skeleton.
- `ui/src/adapters/about_dialog.ts` — opening, focus, Escape, close (role: adapter).
- `ui/src/controllers/app.ts` — the menu-event listener wiring.
- `ui/test/adapters/about_dialog.test.ts`, `e2e/` — the dialog's tests.

## Definition of done

- [ ] The three measurement questions are answered against a real NVDA and written into this
      spec's opening section, including which of the three outcomes obtains.
- [ ] The menu definition is a pure value with tests over structure, labels and ids, and a
      test that every id an item can emit is handled.
- [ ] The About dialog opens from the menu, is announced as a dialog, reads its four facts,
      traps focus, closes on Escape, and leaves focus in the edit field.
- [ ] Version and copyright come from the build; changing the workspace version changes what
      the dialog says with no second edit.
- [ ] Exit closes the window and the shell goes with it.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests, vitest and the
      E2E suite all clean.

## Accessibility checklist for the PR body

Agent-observable through the screen-readers bridge, `user` persona, and recorded inline with
the NVDA version and capture mode:

- [ ] With focus in the edit field, the menu bar can be opened from the keyboard, and NVDA
      announces it as a menu bar.
- [ ] Arrow keys move between Acter and About and into their items, each announced with its
      name and its position.
- [ ] Enter on About Acter opens the dialog, and NVDA announces a dialog with its name.
- [ ] The dialog's four facts are readable with ordinary reading commands.
- [ ] Escape closes the dialog and NVDA announces the edit field as the new focus.
- [ ] Tab from inside the dialog does not escape it.
- [ ] Exit quits, and no `cmd.exe` is left running afterwards. **Human-verified**: this one
      is about a process on the machine after the window is gone, which the bridge cannot
      observe.
