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

## The design, rewritten around what was measured

**Agreed in conversation 2026-08-24**, after the section above turned out to invalidate the
design this spec was written for. DESIGN's native-menu decision is revised in the same
change; what follows is what shipped.

### 1. The menu bar is a WAI-ARIA `menubar` in the page

The static structure lives in `main_window.html` so it is inspectable without executing
anything, and the keyboard behaviour lives in one adapter. Roving tabindex, so Tab reaches
the bar and then leaves it rather than walking every item of every menu.

**Wrapped in `role="application"`**, which is the answer to the objection the old decision
raised and accepted: NVDA reads a document in browse mode, where the arrows move its own
cursor rather than reaching a widget, and it switches to focus mode by itself only if the
user has left that setting on. A menu bar whose arrows depend on somebody's configuration
is not a menu bar. The wrapper covers the bar and nothing else.

### 2. Two ways in: F10, and Alt on its own

F10 is the platform's own "give me the menu bar" and is unambiguous. Alt is what a Windows
user's hands already do, and it cannot be answered on keydown — at that moment Alt+F4,
Alt+Tab and Alt alone are identical. So it is answered on **keyup**, armed by an Alt
keydown with no other modifier and disarmed by any other key, a click, or the window losing
focus. Both leave the bar as well as enter it.

### 3. The command line is deliberately not an application

Tried, measured, reverted the same day. Wrapping it does force focus mode, so typed letters
are text rather than quick-navigation — but it also removes the browsable document from the
one place a listener uses browse mode on purpose: turning focus mode off at the edit field
and arrowing up to hear the tail of the last command. Reading the output wins. The results
buffer is a document for the same reason and more obviously.

### 4. Windows only

The whole menu bar is gated on a `platform` command answering `std::env::consts::OS`, and
the region is removed outright elsewhere. A native menu bar is right on macOS, where menus
live in the system bar rather than in the window; this exists because Windows is where a
native one freezes the reader.

### 5. The web inspector is off

`devtools: false` on the window. Found by the user pressing F10 and landing in dev tools:
wry enables the inspector in debug builds and its keys win before the page sees them.

### 6. Tab is trapped in the dialog explicitly

Measured through NVDA: in a modal `<dialog>` with a single focusable control, Tab leaves it
for the dialog's own document and NVDA drops back into browse mode, and it then took a
second Escape to get out. The dialog cycles focus itself rather than trusting the element.

## What can and cannot be tested

**This section is superseded by the redesign, and the change is entirely in its favour.**
It said that `MockRuntime` does not execute native webview libraries, that WebDriver drives
the webview only, and that therefore **no suite in this project could open this menu** — so
the menu had to be split into a pure value plus a thin construction layer verified only by
a manual NVDA pass.

With the menu bar in the document, all of that goes away: the bar and the dialog are
ordinary DOM, so vitest drives the keyboard contract directly and WebDriver can drive the
whole path end to end. Twenty-one vitest tests cover the bar's navigation and the dialog's
two Acter-specific behaviours; what remains for the NVDA pass is what only a reader can
answer — that the announcements are right and that nothing freezes.

## Decisions

### 1. Two menus, and About holds one item

- **Acter** — Exit. (Connect is added by A8, the day it works — as a dialog since
  2026-08-24, not a submenu. A menu item that opens nothing is worse than a menu that does
  not offer it yet.)
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

**Superseded by the redesign, and for the better.** This described `on_menu_event` in the
composition root emitting one event per menu action to the webview, because the alternative
— Rust opening dialogs — would put user-interface behaviour on the wrong side of the seam.
With the menu bar in the document there is no seam to cross at all: the frontend owns the
menu and what it opens, and Rust owns only the two facts behind it (`about` and
`platform`) plus closing the window.

## Files touched

**Rewritten with the design.** `crates/acter-app/src/menu.rs` never existed: with no native
menu there is no menu definition in Rust to keep pure, and nothing for `on_menu_event` to
route.

- `crates/acter-app/src/routers/about.rs` — the command answering name, version, copyright
  and licence, read from the build (new).
- `crates/acter-app/src/routers/platform.rs` — the command answering which operating system
  this is, so the menu bar is Windows-only (new).
- `crates/acter-app/src/container.rs` — registering those two commands, and nothing else.
- `crates/acter-app/tauri.conf.json` — `devtools: false`, so F10 belongs to the page.
- `ui/src/views/main_window.html` — the menu bar's static structure and the About dialog's.
- `ui/src/adapters/menu_bar.ts` — the menubar keyboard contract, F10 and the Alt trap (new;
  role: adapter).
- `ui/src/adapters/about_dialog.ts` — filling, opening, trapping Tab, returning focus (new;
  role: adapter).
- `ui/src/ports/app_shell.ts` — what the menu needs from the application shell rather than
  from the session (new; role: port).
- `ui/src/routers/tauri.ts` — `TauriShell`, the one module that may import `@tauri-apps/api`.
- `ui/src/main.ts` — the wiring, including the platform gate.
- `ui/src/styles.css` — the bar, kept plain.
- `ui/test/adapters/menu_bar.test.ts`, `ui/test/adapters/about_dialog.test.ts` — their
  tests (new).

## Definition of done

- [x] The three measurement questions are answered against a real NVDA and written into this
      spec's opening section, including which of the three outcomes obtains. **Done
      2026-08-24, and none of the three outcomes obtains**: Alt reaches the menu and the
      menu reads correctly, but opening it freezes the reader for tens of seconds. See the
      section above; the rest of this spec is blocked on that.
- [x] The menu bar's keyboard contract is tested directly — fourteen vitest tests over
      entering, walking, choosing and leaving, including that every leaf runs an action,
      which is the assertion that catches an item nobody wired.
- [x] The About dialog opens from the menu, is announced as a dialog, reads its four facts,
      traps Tab, closes on Escape, and leaves focus in the edit field. Measured through
      NVDA 2026-08-24 and pinned by seven vitest tests.
- [x] Version and copyright come from the build; changing the workspace version changes what
      the dialog says with no second edit.
- [x] Exit closes the window and the shell goes with it. **Measured 2026-08-24** against a
      debug build with `ACTER_SHELL=cmd.exe`: the app spawned `cmd.exe` as its child, a
      `WM_CLOSE` to the window ended the app, and the child was gone three seconds later.
      What that verifies is the close *path* — the Exit item calls
      `getCurrentWindow().close()`, which is the same one. Pressing the item itself from a
      reader is still the checklist's, because the E2E suite cannot assert on an
      application it has just told to quit.
- [x] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests and vitest are
      clean (95 frontend tests).
- [x] The E2E suite drives the menu bar and the dialog end to end, which this design makes
      possible for the first time: eight WebDriver tests over F10, Alt-alone and its
      disarming, walking the bar, opening About, the four facts arriving from the Rust
      side, Tab staying inside, and focus returning. The existing axe audit passes with
      the bar in the document.
- [x] The stray `desconhecido` (unknown) utterance heard once between activating About
      Acter and the dialog announcing itself is understood and fixed: activating a leaf
      moved focus to the edit field *before* running the action, so focus landed there for
      one frame on its way into the dialog and the reader announced that frame. Focus now
      falls back to the edit field only if the action did not take it somewhere, and both
      halves are pinned by tests. **Not yet re-measured through NVDA.**

## Accessibility checklist for the PR body

Agent-observable through the screen-readers bridge, `user` persona, and recorded inline with
the NVDA version and capture mode:

All of the following were **agent-observed** through the bridge on 2026-08-24, NVDA
2026.1.1, `user` persona, live and silent capture, against a debug build of the shipped
design, unless the item says otherwise.

- [x] With focus in the edit field, the menu bar can be opened from the keyboard, and NVDA
      announces it. **F10 and Alt-alone both**: `Acter menu bar` then
      `Acter subMenu 1 de 2`, immediate.
- [x] Arrow keys move between Acter and About and into their items, each announced with its
      name and its position — `About subMenu 2 de 2`, `Exit 1 de 1`, `About Acter 1 de 1`.
- [x] Enter on About Acter opens the dialog, and NVDA announces a dialog with its name:
      `About Acter diálogo` (this reader is running in Portuguese).
- [x] The dialog's four facts are read: `Acter, Version 0.1.0, © 2026 Marlon Brandão de
      Sousa, MIT licence`, followed by `Close botão`.
- [x] Escape closes the dialog and NVDA announces the edit field as the new focus:
      `Command input edição em branco`.
- [x] Tab from inside the dialog does not escape it. **Failed first**: focus left the Close
      button for the dialog's document (`Acter, DOCUMENT`), NVDA dropped into browse mode
      and a second Escape was needed. Fixed, re-measured, and pinned by a test.
- [ ] The stray `desconhecido` heard once on activation is gone. **Fixed but not
      re-measured**: the cause was a one-frame focus stop in the edit field on the way into
      the dialog, removed and covered by tests, and this box stays open until a reader has
      confirmed it.
- [x] Exit quits, and no `cmd.exe` is left running afterwards. **Agent-verified by process
      inspection rather than by ear** (2026-08-24): the window's close path was driven and
      the spawned `cmd.exe` was confirmed gone. Pressing the menu item with a reader is
      still worth one human pass, because the bridge cannot
      observe.
