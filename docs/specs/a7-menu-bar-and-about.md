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

### What was measured, 2026-08-24

**How.** A spike build: two `SubmenuBuilder` menus (Acter with Exit, About with About Acter)
attached to the main window with `window.set_menu`, and an `on_menu_event` handler printing
the id it received. Nothing else changed, and the spike was reverted once the questions were
answered — this section is what it left behind. Driven through the screen-readers bridge as
the `user` persona, NVDA 2026.1.1, silent capture, both a debug build and a release build
(`--release --features custom-protocol`, so no WebDriver plugin and no debug recorder).

**Question 1: yes, Alt reaches the menu bar.** With focus in the Command input — in browse
mode and in focus mode alike — pressing Alt activates the native menu bar. NVDA lands on the
first item and reports it as `MENUITEM` with `HASPOPUP` and `FOCUSED`. Microsoft's
accelerator caveat is about *accelerators* (Alt+X while the webview has focus); the bare
`WM_SYSKEYDOWN` that opens a menu bar is not one, and it is not swallowed. **So question 3
was never reached**, and the `Ctrl+Shift+M` fallback is not needed.

**Question 2: announced correctly, and unusable anyway.** What NVDA says is right: `Acter
subMenu Alt+ a` on the bar, `Exit e` and `About Acter a` inside, right and left arrow walk
the bar, down arrow opens a submenu, Enter fires the item — `on_menu_event` received
`MenuId("about-acter")` — and Escape closes the menu and returns focus to the edit field,
which NVDA announces along with the document's landmarks. Every one of those steps is
sub-second.

**The first announcement is not.** Between the Alt press and NVDA naming the menu:

- Debug build: **68 s**, **45 s**, **about 18 s** on three separate presses.
- Release build: **36 s** and **20.5 s**. One press answered in about a second — the very
  first Alt after the window opened, before NVDA had built a virtual buffer for the
  document, which is the one case that was ever fast.

**And it is the reader that stops, not just the announcement.** Throughout the stall NVDA's
main thread is unresponsive: every bridge call that has to marshal to it times out, and
keys pressed in the meantime queue up and land when it recovers. A listener does not meet a
slow menu; they meet a screen reader that has died for half a minute, with no way to know
whether the menu opened.

**Three controls, because a finding this size is usually the instrument rather than the
software.** None of them explains it away:

- *Same reader, same bridge, a classic Win32 menu bar* — System Information (`msinfo32`):
  Alt answered `File subMenu Alt+ f` immediately. Not the machine, and not NVDA in general.
- *Release build*: stalls too, so it is not the debug WebDriver server or the debug event
  recorder — the two things a debug build injects that a release build does not.
- *A keystroke from outside NVDA*: a real Alt injected by `SendKeys` from a separate process
  rather than by the bridge stalled **20.5 s** and produced the same unresponsive reader. So
  it is not an artifact of how the key was pressed.

**The one control not run at first** — the human pressing Alt with their own hand — was run
next, and it is what produced the diagnosis below.

### What NVDA's own log says, 2026-08-24

**It is not Acter's code, and it is not how the spike attached the menu.** A deviation was
found while writing this up — these spikes call `window.set_menu(menu)` on an existing
window, where Tauri's tutorial calls `app.set_menu(menu)` inside `setup` — so a **vanilla
Tauri application was built outside this repository from that tutorial's code, unedited**,
with a three-element page of its own and no Acter anywhere in it. It freezes exactly the
same: the user by hand, and then measured through the bridge, **Alt pressed 08:58:21,
`Open Alt mais o` spoken 08:58:55.9 — about 35 seconds.**

**And the freeze is the menu, not focus leaving the webview.** In that same window, in the
same session, seconds earlier: **Alt+Tab out to another application announced the new window
in about a second**, and Alt+Tab back the same. Reading inside the page — arrows, Tab into
the field — is sub-second throughout. So a webview losing focus is fine; a *native menu
taking* it is not.

**A mechanism that fits, offered as a hypothesis rather than a finding.** Opening a menu bar
puts the window's UI thread into Windows' modal menu loop — the same thread that hosts
WebView2. NVDA's accessibility queries into that window and its renderer then wait on a
thread that is not servicing them, and each one blocks until COM's message filter cancels it
at about ten seconds, which is exactly the three cancels the log shows. Alt+Tab does not
enter that loop, which is why it is clean.

**Prior art, all NVDA-side and all the same shape**: a freeze whose stack is
`treeInterceptor cleanup` → `_get_isAlive` (nvaccess/nvda#4572), a fix catching `COMError`
when testing containment in a *dead document* in `UIAHandler/browseMode`
(nvaccess/nvda PR #15736), a Chromium-plus-UIA freeze recovered by the watchdog
(nvaccess/nvda#18239), and WebView2's own long-running screen-reader complaints
(MicrosoftEdge/WebView2Feedback#2330). None of them is filed against a native menu, which is
what this measurement adds.


**Captured by the user**, pressing Alt by hand in a fourth build: a window carrying the menu
bar and **an empty page** — `about:blank`, no session, no frontend, `WRY_WEBVIEW`, virtual
buffer eleven characters long. It stalls exactly like the real window, so **the stall is not
Acter's page**: not the terminal buffer, not the live region, not the landmarks.

**What the reader was doing during the silence.** NVDA's watchdog recorded three consecutive
freezes of its main thread — 10.5 s, 10.1 s and 9.9 s, about thirty seconds in total — and
the stacks name the call each time. All three are synchronous accessibility calls *into the
WebView2 renderer process* (`msedgewebview2`), made because focus was **leaving** the webview
document, which is precisely what opening a native menu does:

1. `doPreGainFocus` → `setFocusObject` → a `loseFocus` handler → `obj.name` →
   `_get_IAccessibleRole`, ending in
   `accRole failed: (-2147418110, 'A chamada foi cancelada pelo filtro de mensagem.')` —
   `RPC_E_CALL_CANCELED`: the call never answered and COM's message filter cancelled it after
   ten seconds.
2. `treeInterceptorHandler.cleanup` → `virtualBuffers/gecko_ia2._get_isAlive`, asking the
   same dying document whether its buffer is still alive.
3. `_get_treeInterceptor` → `UIAHandler/browseMode.__contains__`.

Immediately after the third recovery, at `08:38:15.863`, NVDA speaks
`['Acter', 'subMenu', 'Alt+', 'a']`. **The announcement was never slow**: it was queued
behind the freezes, which is why everything after the menu opens is instant.

**The menu bar itself is exposed correctly**, which the same log shows:
`name: 'Aplicativo', role: MENUBAR, event: focusEntered, window class name: Tauri Window`,
then `Acter`, `MENUITEM`, `gainFocus`. Nothing is wrong with what Tauri builds.

**A confound this measurement cannot dismiss, named rather than buried.** The tester's NVDA
runs third-party add-ons, and two of them are in the stacks: the first freeze is entered from
**evtTracker**'s `loseFocus` debug handler asking the webview document for its `name`, with
**YoutubePlus** wrapping `setFocusObject` beneath it, and the capture was taken at debug log
level. So the first ten seconds are provoked by an event-tracing add-on. Freezes 2 and 3 are
in NVDA's own code, so the stall does not obviously vanish without them.

**And it does not: the user re-ran it with the add-on disabled and debug logging off, and
the thirty seconds did not collapse.** So the confound is closed, and it was not the cause.
What an ordinary listener meets is what was measured: a reader frozen for tens of seconds
by opening a menu.

**What this means for the design, stated rather than decided here.** The measurement does
not land on any of the three outcomes this section anticipated: question 1 passes, so the
`Ctrl+Shift+M` fallback is unnecessary, and question 2 fails on something the questions did
not think to ask — not *whether* the menu is announced but *when*, and not because of
anything Acter draws. **A7 does not proceed on the strength of what is written below**, because
the add-on-free measurement was taken and the cost is real for an ordinary listener.

**It is not ours to fix, and that is what decides the entry.** The blocking calls are
NVDA's, into a renderer process Acter does not own, and the smallest possible Tauri
application does it too. What is within reach is only the choice of *whether the menu bar is
native at all* — and that choice now has a measurement behind it rather than a preference.
**DESIGN's native-menu decision is reopened**: an in-document menu bar keeps focus inside
the webview, where nothing about this freeze applies, at the cost of the ARIA authoring that
DESIGN chose a native `HMENU` to avoid. Both halves of that trade are now known, which is
what this section was for.

**Worth reporting upstream**, with this spec's numbers and NVDA's stacks: the vanilla
reproduction is twenty lines from Tauri's own tutorial, and neither Tauri nor NVDA appears
to have this case on file.

**One design note, independent of the stall.** Both top-level menus announce the same
accelerator — `Acter subMenu Alt+ a` and `About subMenu Alt+ a`. With no mnemonic in the
labels, the platform gives both the same first letter, so Alt+A is ambiguous. Decision 1's
labels need explicit, distinct mnemonics.

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

- **Acter** — Exit. (The Connect submenu is added by A8, the day it works. A menu item that opens
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

- [x] The three measurement questions are answered against a real NVDA and written into this
      spec's opening section, including which of the three outcomes obtains. **Done
      2026-08-24, and none of the three outcomes obtains**: Alt reaches the menu and the
      menu reads correctly, but opening it freezes the reader for tens of seconds. See the
      section above; the rest of this spec is blocked on that.
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
