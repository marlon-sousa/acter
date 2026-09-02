# M3 — the menu bar macOS actually has

Roadmap entry 34, lane 3. DESIGN has said since A7 that on macOS a menu belongs in the
system bar and not in the window; the frontend honours the second half of that already and
nothing honours the first. This entry gives a Mac the menu the platform expects, with the
one item this product exists for in it.

## What is true today

`main.ts` asks the backend which platform this is, and off Windows it removes the
`menu-bar-region` outright rather than hiding it. So the WAI-ARIA `menubar` A7 built, F10,
and Alt-on-its-own are Windows-only, which is what A7 decided and is not what this entry
changes.

**The board said "nothing in its place", and that was wrong.** Tauri 2.11.5 installs a
default macOS menu whenever the builder was given none — `Builder::build` sets
`Menu::default(app_handle)` when `menu.is_none() && enable_macos_default_menu`
(tauri-2.11.5/src/app.rs:2245). A Mac running Acter therefore has a menu bar already, and
the reason to replace it is what is in it rather than that it is missing.

## What was measured, and where

**On the developer's Mac, 2026-09-02**, macOS 15 (Darwin 24), against the real debug
binary — unbundled, launched from a terminal — with the menu read **out of the
accessibility tree** through System Events rather than looked at, so what is recorded is
what an assistive client is offered.

- **Six menus**: Acter, File, Edit, View, Window, Help.
- **The app menu is named after the binary, and nothing in Acter can change that**: the
  bar reads `acter-app`, and its items read `About acter-app`, Services, `Hide acter-app`,
  Hide Others, `Quit acter-app`. **Proved rather than inferred**: the first submenu's title
  was set to `ActerZZ` and rebuilt, and the bar still read `acter-app` — AppKit takes the
  application menu's name from the process, and the words in the predefined items come from
  the same place. So this is a fact about the *bundle*, M4 is the entry that produces one,
  and decision 4 leaves those words to the platform rather than writing them here.
- **Help is empty.** `Menu::default` puts the About item in Help only
  `#[cfg(not(target_os = "macos"))]`, so on a Mac the submenu is built with no items at
  all. A listener who opens Help arrives somewhere that says nothing.
- **Connect is in no menu.** File holds `Close Window` and `Close All`. The one control
  this product exists for is reachable on a Mac only from the button in the unconnected
  window, and not at all once a session is running.
- **Edit is there** — Undo, Redo, Cut, Copy, Paste, Select All, plus the AutoFill,
  Dictation and Emoji items macOS injects — which is why copy and paste work in the
  webview today. On macOS those keys are routed by the menu, so an Edit menu is not
  decoration.

**Two behaviours checked rather than assumed**, because either answer would have changed
the design:

- **Closing the window ends the process.** `File > Close Window` on the running app left no
  `acter-app` behind. macOS conventionally keeps an application alive with no windows, so
  `AppShell.exit`'s "close the window" (A7, decision 2) had to be confirmed to still mean
  what it says here. It does.
- **The platform's own Quit takes the shell with it.** Launched with `ACTER_SHELL=/bin/zsh`,
  the app had one child, `zsh`; choosing `Quit acter-app` ended the app and the child was
  gone. So the predefined quit item does not orphan a session, which is what decision 3
  rests on.

**And what the finished menu does, measured the same way on the same day** — the real
window, items chosen through the accessibility API, and the answer read back as *where focus
landed*, which is the question a listener's experience turns on. No screen reader was
involved: this says the path works, and the PR's checklist is where it is judged as speech.

- **Acter Help** opened the help topic with focus on its first section's heading, "What
  Acter is", heading level 2 — the same landing F1 gives.
- **Connect…** opened the Connect dialog with focus on `Terminal` in the connection-kind
  list, from a window with no session.
- **About Acter** opened the About dialog.
- **Cmd+K** opened the Connect dialog on the same landing, from the window.
- **Cmd+Q** with a live `/bin/zsh` ended the application and left no shell behind, this time
  through Acter's own menu rather than Tauri's default one.
- **F1 still opens the help**, unchanged, with the native menu installed.
- **Cmd+? did not open the help**, which is decision 6's finding: it put focus in the search
  field macOS injects into every Help menu. `Cmd+/` opens the topic, and is what shipped.

**One measurement that belongs to the help rather than to the menu**: this Mac has no
`com.apple.keyboard.fnState` default, so the system's factory answer applies and **F1, F6
and F10 need fn**. Acter's window keys are all function keys. That is M3.5's to write down,
and it is why the two items this menu carries get accelerators of their own (decision 6).

## Decisions

### 1. The menu is a value, and the operating system is an argument

`policies/system_menu.rs`, exactly as `catalogue.rs` does it since M1: `system_menu(os)`
returns the submenus that operating system's own menu bar holds, and every platform's
answer compiles and is asserted on every platform. Windows and Linux answer with an empty
list — Windows because its menu is in the document and a native one freezes NVDA for tens
of seconds (A7), Linux because it has no answer yet and an empty one is honest.

**The empty list is what keeps the Windows build safe**, and it does it structurally: the
composition root calls `Builder::menu` only when the list is non-empty, so there is no
platform where Acter attaches a native menu that nobody decided to attach. It also turns
off Tauri's default — a menu that was set is not replaced by `Menu::default`.

### 2. Six submenus, and the two that are not strictly earned stay

DESIGN's rule is that a menu bar entry has to earn its place, and on Windows that gave two
menus. macOS is not the same room: the system augments a menu bar it recognises — the
Window menu collects window commands, Help gets the system's help search — and an
application whose menu bar is missing the entries every other application has is a
different kind of surprise from one carrying an entry nobody needs.

- **Acter** — About Acter, Services, Hide, Hide Others, Show All, Quit.
- **File** — Connect…, Close Window.
- **Edit** — Undo, Redo, Cut, Copy, Paste, Select All. Kept because macOS routes the
  webview's own copy and paste through it; removing it would take Cmd+C out of the command
  line, which on a terminal for listeners is not a cosmetic loss.
- **View** — Toggle Full Screen.
- **Window** — Minimize, Zoom, Close Window.
- **Help** — Acter Help. This is the submenu that is empty today, and filling it is the
  smallest way in which this entry is a fix rather than an addition.

### 3. About and Help open Acter's own dialogs; Quit is the platform's

**About Acter opens the HTML dialog**, not `PredefinedMenuItem::about`. DESIGN's "dialogs
are HTML modals in the window" was decided for reasons that hold on any platform — the text
is browse-readable and copyable, and both test suites can drive it — and the native panel
would say less: name and version, with the copyright and licence Acter's dialog reads out
absent until a bundle carries them. Acter Help opens the help topic for the same reason and
one more: it is the same topic F1 opens, and two ways into one text is the point of A13.

**Quit is `PredefinedMenuItem::quit`**, with Cmd+Q. It is what a Mac user's hands already
do, and the measurement above says it does not leave a shell running. The alternative —
routing Quit through the frontend's `AppShell.exit`, one exit path on both platforms — buys
symmetry at the cost of not being the platform's quit, and the risk it was meant to cover
turned out not to exist.

### 4. Acter writes its own items' words, and the platform keeps writing the platform's

The first draft of this decision said the opposite — that the layout would carry the label
for the standard items too, so that "Quit acter-app" became "Quit Acter" — and it was wrong
for a reason that only shows on the machine this product is being built on: **macOS
localises those items**, and the account here runs in Brazilian Portuguese. Passing English
text through to `PredefinedMenuItem::quit` would replace a translation the system already
has with a string in a language the user did not choose, and it would do it to Cut, Paste
and Minimise as well.

So Acter names Connect…, Acter Help and About Acter, which are its own and which no
platform has an answer for, and the standard items are built with `None` — the system's
words, the system's language, the system's conventional shortcut.

**The cost is a name that is wrong until M4.** An unbundled build's process name is
`acter-app`, so the app menu currently reads "Quit acter-app". That is a fact about the
bundle rather than about the menu: bundling gives the process the product's name and those
items come right in every language at once, which hard-coding one language here would not
do. It is recorded in M4's entry rather than worked around in this one.

### 5. A menu choice reaches the frontend as a protocol value

The three items Acter answers itself — Connect, Acter Help, About Acter — are custom items,
and choosing one emits a single event carrying a `MenuAction`. `MenuAction` is an
acter-core value in the generated bindings, so the frontend's switch over it is exhaustive:
a variant added to the menu with no dialog behind it fails to compile, which is the rule
`ConnectQuestion` already holds the frontend to.

The Rust side maps `MenuAction` to a menu id and back, and that round trip is the only
logic in the adapter; the rest is a builder call chain.

**The frontend routes it to the same actions the document menu bar is given.** `MenuActions`
already exists and already knows nothing about menus; the system menu is a second caller of
it, so Connect from a Mac's File menu and Connect from Windows' F10 menu are one path.

### 6. Connect and Help get accelerators, because on a Mac the function keys are not free

`Cmd+K` for Connect is the platform's own "connect to server". F1 and F6 keep working
exactly as they do — nothing here rebinds them — but on a Mac with factory settings they
need fn, which is measured above, so the menu's accelerators are the ones that work with no
system setting changed.

**Help is `Cmd+/`, and the conventional `Cmd+?` was tried first and measured failing.**
macOS injects a search field into any menu titled Help and reserves ⇧⌘/ for it, and that
binding wins: with Acter Help bound to ⇧⌘/, pressing it put focus in the system's search
field and the help never opened — an accelerator the menu advertises and does not run. ⌘/
is unclaimed, reaches the application and opens the topic at its first section, which is
what F1 does. A working shortcut that is not the convention beats a conventional one that
lies, most of all when the alternative is a function key that needs fn.

### 7. What can be tested here, and what a test cannot reach

The layout policy is a value and is tested on every platform, including the assertions this
entry's measurements produced: Help is not empty, Connect is in the menu, every action is
reachable exactly once, no label is blank, no shortcut is a lone function key, and Windows
and Linux ask for no native menu at all. The id round trip
is tested both directions. The frontend's routing is tested with a fake event source.

**Building the NSMenu is not reachable from a test**, and the spec says so rather than
letting a green suite imply otherwise: menus are created through muda against the real
application on the main thread, and `MockRuntime` has no such thing. The evidence that the
menu is right is the accessibility-tree reading in this document and the VoiceOver checklist
in the PR body — which is the same standard A7 held its own menu bar to before WebDriver
could reach it.

### 8. Text that belongs to one platform is removed, not hidden

The help says "F10 opens the menu bar, and so does pressing and releasing Alt on its own",
which is false on a Mac and is now false in a new way: there *is* a menu bar there, in a
different place, reached with a different key. So one sentence has to differ by platform,
and the mechanism is `data-platform` on the element plus one adapter that removes what does
not match, at startup, before anything is announced.

It is removal rather than `hidden` because that is what `main.ts` already does with the
menu-bar region, for the reason A7 gives: nothing empty is left in the document for a
listener to meet. The region itself moves onto the same mechanism, so the frontend has one
answer to "this belongs to Windows" instead of two. M3.5 is what fills the mechanism up.

### 9. Where focus is after a menu item is chosen

The document menu bar's rule (menu_bar.ts) is that the action places focus, and only if it
placed none does the bar put it back where the window keeps it. The system menu takes the
same rule and the same fallback, because the failure it protects against is the same: a
dialog that opens takes focus itself, and an action that opens nothing must not leave a
listener standing on a menu that has closed.

## Files touched

- `crates/acter-core/src/entities/menu_action.rs`: `MenuAction`, the protocol value.
- `crates/acter-core/src/policies/system_menu.rs`: `system_menu(os)`, the submenus, their
  items and every word in them.
- `crates/acter-core/src/entities.rs`, `policies.rs`, `lib.rs`: the facade lines.
- `crates/acter-app/src/adapters/macos_menu.rs`: the adapter that renders the layout into
  Tauri's menu, and turns a chosen item into an emitted `MenuAction`.
- `crates/acter-app/src/adapters.rs`, `container.rs`: the gated line and the one call.
- `crates/acter-app/tests/protocol_bindings.rs`: `MenuAction` registered.
- `ui/src/ports/system_menu.ts`: what the frontend needs
  from the operating system's menu — a stream of chosen actions and nothing else.
- `ui/src/adapters/system_menu.ts`: the routing, and the focus fallback.
- `ui/src/adapters/platform_text.ts`: decision 8.
- `ui/src/routers/tauri.ts`: `TauriSystemMenu`, the one place that listens.
- `ui/src/main.ts`: the composition, with the menu-bar region's removal moved to
  `data-platform`.
- `ui/src/views/main_window.html`: `data-platform` on the menu-bar region and on the
  sentence about F10.
- `ui/src/protocol.ts` (generated).

## Definition of done

- A Mac's menu bar holds six menus — the application's own, File, Edit, View, Window and
  Help; File holds Connect and Help holds Acter Help. The application menu is named by the
  process until M4 bundles it, which is recorded rather than worked around.
- Connect from the menu opens the Connect dialog whether or not a session is running; About
  and Acter Help open the dialogs F1 and the Windows menu open.
- Cmd+K connects, Cmd+/ opens help, Cmd+Q quits and the shell it was running goes with it.
- Windows is untouched: the document menu bar, F10 and Alt behave exactly as they did, and
  no native menu is attached.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets`, the UI suite and the
  E2E suite green.
- The VoiceOver checklist in the PR body, each item marked agent-observed or human-verified.

## Accessibility checklist (PR body)

One item per check, findings written inline on the unchecked item, naming the VoiceOver
version and the capture mode.

1. VoiceOver reaches the menu bar and announces the six menus by name.
2. Opening a menu is immediate — no stall of the kind A7 measured on Windows, and the
   number is recorded either way.
3. File announces Connect, and choosing it opens the Connect dialog with the dialog
   announcing itself.
4. Help announces Acter Help, and choosing it lands on the first section of the topic —
   the same landing F1 gives.
5. About Acter announces the dialog and reads name, version, copyright and licence.
6. Cmd+K from the window opens the Connect dialog; Cmd+/ opens help.
7. After a menu item that opens nothing, focus is somewhere announced rather than on a
   closed menu.
8. Cmd+C in the command line copies, with the Edit menu present.
9. The window's own keys are unchanged: fn+F1 opens help, fn+F6 moves between the areas.
10. Nothing in the flow depends on a sound cue rather than speech. (Human.)
