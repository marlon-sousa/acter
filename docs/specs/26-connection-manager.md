# Spec: PR 26 — the connection manager, and where Acter keeps its settings

Connections get names and are saved. The Connect dialog becomes a list of those names, each
loading its own properties into a panel a user can change before connecting. A new
connection is made in a dialog of its own, and once it is up Acter offers to save it, once.
Nothing here ever saves a password. Underneath, everything Acter writes moves into one
settings folder, which is beside the program when Acter runs portable and in the user's own
application-data folder when it is installed.

Agreed in conversation 2026-09-12. It supersedes
[b8-profile-store.md](b8-profile-store.md), which was agreed 2026-08-23 and never
implemented; that file stays, with a note at its top pointing here, because the reasoning it
records for JSON, for one file per connection and for a named failure still holds and is
reused below.

## Why now / relation to the roadmap

- Roadmap entry 26, lane 2, resequenced on 2026-08-24 to become "saved connections" and
  still open. Every one of its prerequisites is Done: the kinds are discovered (B5.4, B5.7),
  WSL lists its distributions (23.3), SSH exists (27), and the Connect dialog has a panel per
  kind (A8).
- **Two settings have been waiting for this entry to exist.** The set-up checkbox "travels
  with the attempt rather than being stored, until B8 has a profile to keep it in" (B9.5,
  decision 10), and 28.8 parks "who gets your keys is not remembered per connection" until
  a saved connection can remember it. Both are answered here without a setting of their
  own: saving writes the session as it stands.
- **It is the first of the 1.0 beta set**, and the settings folder is in it because the
  connection store is the first thing Acter writes that a user would go looking for.
- It touches lane 3 only where the settings folder lives on macOS and where a portable copy
  writes there. Bundling, signing and the installer for macOS stay in entry 35 (M4).

## What exists today, measured in the code

- `ProfileId` (`crates/acter-core/src/entities/protocol_commands.rs`) is the typed thing a
  session is started from: `Shell`, `Install`, `Distribution`, `Program`, `Ssh`, `Scripted`.
  It crosses the wire and is handed back unchanged by the dialog.
- `ConnectApi` (`crates/acter-core/src/ports/driving/connect_api.rs`) has three methods:
  `connectable`, `use_profile` and `connected`. `use_profile` takes the `SetUp` choice and
  the questions the attempt may ask; `Connected` carries the session id, its label and a
  note.
- The Connect dialog (`ui/src/adapters/connect_dialog.ts`) is a list of kinds with a panel:
  SSH gets a three-field form, WSL and PowerShell get a list of variants, cmd gets "no
  options". Its set-up checkbox, its Help button and the connecting dialog underneath it are
  reused here unchanged.
- The set-up dialog's "do not show this dialog again" is a `ConnectAnswer::SetUpSession`
  with `remember`, kept per shell by `ExplainedShells` in a plain text file.
- `container.rs` resolves one directory for Acter's own records: `ACTER_PROFILES_DIR` if
  set, otherwise `%APPDATA%\acter` on Windows and `~/Library/Application Support/acter` on
  macOS, otherwise the working directory. Two files live there: `known_hosts` and
  `explained_shells`. `--profile` is retired in a comment and parses nothing.
- `bundle.active` is `false` in `tauri.conf.json`, so there is no installer on any platform.

## Design decisions

### The vocabulary is saved connections, and the folder is settings

1. **The word is connection.** The product's own dialog is called Connect and the thing a
   user names is what they connect to. "Profile" survives only inside the code, as
   `ProfileId`, which is not renamed here because it is a type on the wire and this spec has
   enough surface. `--profile` becomes `--connect <name>`. `ACTER_PROFILES_DIR` becomes
   `ACTER_SETTINGS_DIR`; nothing shipped reads the old name, so there is no alias.

### Where Acter keeps its settings

2. **One settings folder, and everything Acter writes goes in it.** Today's two files move
   there and the new ones join them:
   - `connections/<name>.json`, one file per saved connection;
   - `known_hosts`, Acter's own record of accepted host keys, unchanged in format;
   - `explained_shells`, unchanged;
   - `preferences`, plain text, one `key=value` per line, holding the one preference this
     spec adds (decision 19). Same reasoning as `explained_shells`: inspectable and
     deletable with tools the user already has.

3. **Portable or installed is decided when the binary is built, and never guessed at when it
   runs.** The packaging is the fact — an installer put a copy somewhere and a zip did not —
   so the package declares it and the running program asks nothing of the filesystem. A
   Cargo feature on `acter-app`, `portable`, off by default: an ordinary build and the
   installer's build are the same thing, and only the zip's build is compiled with it
   (decision 21). The settings folder is then:
   - portable: `settings` beside the program. On Windows, beside `acter.exe`. On macOS,
     beside the `.app` bundle rather than inside it, because a file written inside the
     bundle breaks its signature the moment M4 signs it;
   - installed, Windows: `%APPDATA%\acter\settings`;
   - installed, macOS: `~/Library/Application Support/acter/settings`.

   **Amended in implementation, 2026-09-12, before any of it shipped.** This decision first
   said the gate was the *presence* of a folder named `settings` beside the program, and the
   user rejected it on reading the implementation: it infers a fact about the package from a
   side effect on disk. A folder called `settings` is a thing plenty of directories happen to
   contain — a repository checkout, a shared tools folder, a memory stick another application
   wrote to — and a copy run from one of them would have silently read and written that store
   instead of the user's own. The marker was also the storage, so the accident did not merely
   flip a mode: the saved connections would appear to have vanished, and the only place saying
   otherwise is the About dialog, which a listener has no reason to open. A build-time answer
   has no accident case at all. What it costs is that a user can no longer convert an
   installed copy by hand, which the variable below covers.

   **Acter creates the folder on first write, not at startup**, whichever it is, so a machine
   Acter was only ever run on and never saved anything from has no folder. A portable copy
   that cannot tell where its own program is — which nothing has been observed to do — writes
   in the directory it was started from and says so, rather than inventing a location.

   **`ACTER_SETTINGS_DIR` wins over the packaging**, exactly as `ACTER_PROFILES_DIR` did: it
   is what points development, the suites and the NVDA fixture at a directory made for them,
   and a manual pass whose saved connections depend on this machine's history is not
   repeatable. It is also the one way to keep settings somewhere the packaging did not
   choose, which is what a user converting a copy by hand now does. An empty value is ignored
   rather than treated as a path.

4. **The rule is one pure function, and it is tested for both platforms and both packagings
   without being either.** `records_directory` already takes the operating system and the
   environment as arguments for this reason; it grows the executable's directory and the
   packaging, and the composition root is the only caller that reads the real answers. The
   packaging reaches it as a *value*, from one small `#[cfg]`-gated function beside
   `signatures` and `machine`, so an ordinary `cargo test` covers the portable branch too —
   a `#[cfg]` inside the rule would have compiled half of it out of every test run. Its tests
   cover: portable on Windows, portable on macOS staying outside the bundle, installed on
   both, an installed build ignoring a `settings` folder beside it, the variable winning over
   the packaging, a portable build that cannot find its program, a platform with no answer
   falling back to the working directory as it does today, and the build's own answer matching
   its feature.

5. **The settings folder is said where a user can read it.** The About dialog gains one
   line, "Settings folder:" followed by the path, and whether Acter is running portable or
   installed. This is the cheapest answer to "where did that go", and the About dialog is
   already a readable document.

6. **A write that fails is a sentence, never a silent nothing.** "Could not save the
   connection: the settings folder C:\path is not writable." Naming the folder is the whole
   point; a listener with an unwritable Program Files cannot see a red squiggle.

### What a saved connection is

7. **A name, a target, and the two settings the session has.** JSON, one file per
   connection, with a `version` field from the first file written. The target is tagged by
   kind and carries only that kind's facts:

   - `Cmd`: nothing.
   - `PowerShell`: the edition (Windows PowerShell or PowerShell 7) and the provenance the
     list showed, such as `preview` or `Microsoft Store`. **Not the resolved file path.**
     The path is resolved again at connect time by matching edition and provenance against
     what discovery answers now, so a PowerShell upgrade does not break a saved connection.
     If nothing matches, the connection is listed and not available, with the same
     instructions the variant would carry.
   - `Wsl`: the distribution name, as `wsl.exe -l -q` spelled it.
   - `Program`: the program, as `ProfileId::Program` carries it today. No arguments field
     until something starts a program with arguments.
   - `Ssh`: host, port, account. Never a password, never a passphrase: B9 decision 5 stands.
     A key file path joins this when B9.1 lands, not before.
   - `Terminal`: nothing; the shell this macOS account logs in to.
   - `Scripted`: the scenario name. Saveable in a debug build like any other kind, because
     the fake is a permanent supported session kind (DESIGN) and because the end-to-end
     suite needs a saved connection it can start without a shell. A release build lists it
     as not available, since it never constructs one (B7, decision 7).

   The settings are `set_up` (`Yes` or `No`) and `line_owner` (`Local` or `FarEnd`). Nothing
   else. A starting directory and an auto-read threshold have a place in the format and no
   implementation, and B8's reason stands: settings nothing reads are settings nothing
   tests.

8. **A name is a file name, and the rule is said in one sentence.** The file is
   `connections/<name>.json`, so a name cannot contain the characters slash, backslash,
   colon, star, question mark, double quote, less than, greater than or bar; it cannot be
   empty, and it is compared case-insensitively, because the Windows filesystem does. A
   name that breaks the rule is refused at Save with the sentence "A name cannot contain
   slash, backslash, colon, star, question mark, quote, less than, greater than or bar."
   Spelled out in words, because a screen reader reads the characters themselves
   unreliably.

9. **A file that cannot be read is named, and the rest still load** (B8, decision 5, kept
   whole). One malformed file is one row in the Connect dialog whose label ends in "(could
   not be read)" and whose panel says the file and what was wrong, in words. A missing
   `connections` folder means no saved connections and no error.

### The backend: a store behind a port, and five actions

10. **`ConnectionStore` is a driven port with a filesystem adapter and a fake.**

    ```rust
    pub trait ConnectionStore: Send + Sync {
        /// Every file: the ones that parsed, and the ones that did not, by name and reason.
        fn load(&self) -> ConnectionsLoaded;
        fn save(&self, connection: &SavedConnection) -> Result<(), String>;
        fn rename(&self, from: &str, to: &str) -> Result<(), String>;
        fn forget(&self, name: &str) -> Result<(), String>;
    }
    ```

    Every `String` error is a whole spoken sentence. The adapter is tested against a
    temporary directory; the service is tested through the fake with no filesystem.

11. **`ConnectApi` grows, and `use_profile` learns where a session came from.**

    - `saved(&self) -> Vec<SavedRow>`: every saved connection, freshly read on every call for
      `connectable`'s reason (B7, decision 6). A row carries the name, the `ProfileId` the
      panel is loaded from, `set_up`, `line_owner`, `available`, and `instructions` when it
      is not available or could not be read. Availability is judged against what discovery
      answers now: a distribution that was uninstalled, an edition that is gone, a scripted
      scenario in a release build.
    - `use_profile(&self, id, set_up, origin: Option<&str>, questions)`: `origin` is the
      saved name the attempt started from, or `None` for a new connection. It is the
      frontend's knowledge, because the user may have edited the panel before pressing
      Connect and the backend cannot know which row that came from.
    - `save_connection(&self, name: &str) -> Result<String, String>`: writes the live
      session as it stands, that is its `ProfileId`, its `set_up`, and whoever owns the
      line now, under `name`. If `name` is the session's origin, it replaces. If another
      saved connection already has that name, it refuses: "A connection named X already
      exists. Choose another name, or forget that one first." On success the session's
      origin becomes `name`, and the sentence returned is "Saved as X."
    - `rename_connection(&self, from, to)` and `forget_connection(&self, name)`, each
      answering a sentence. Renaming the live session's origin renames the origin too.
    - `Connected` gains `saved_as: Option<String>` and `line_owner: LineOwner`. The first is
      the origin, so the frontend knows whether to offer saving and what to prefill. The
      second is what the saved connection asked for; the frontend applies it where it
      already decides which owner a new session starts on, and a saved choice wins over the
      default there.
    - `requested_at_launch(&self) -> Option<LaunchRequest>`: what `--connect` asked for,
      see decision 20.

    Everything above is reached through routers exactly as `use_profile` is, and every new
    invoke has a `MockRuntime` test.

### The Connect dialog: a list of names

12. **File → Connect opens a dialog whose main list is the saved connection names**,
    alphabetical, case-insensitive, and stable: a listener learns positions, so the order is
    never most-recent-first. The list's accessible name is "Saved connections". Focus lands
    on the first name, so the everyday case is open, arrow, Enter.

13. **Arrowing onto a name loads its properties into the panel and announces one line.**
    The panel is the same component the New connection dialog uses (decision 17), loaded
    with the row's values: the SSH form filled in, the distribution or edition selected, the
    set-up checkbox as saved. The announcement is the kind and what identifies it: "SSH,
    marlon at example.org", "WSL, Ubuntu", "PowerShell 7", "Command Prompt". A row that is
    not available announces "not available" and its panel holds the instructions, as a kind
    does today.

14. **Edits in the panel apply to this attempt only.** Changing the port and pressing
    Connect connects to that port and writes nothing. Keeping the change is File → Save
    connection afterwards (decision 18). The help says this in one sentence.

15. **Five buttons: Connect, Rename, Forget, New connection, Cancel.** Enter connects from
    anywhere in the dialog, as today. There is no Save here; saving happens from a live
    session and nowhere else, so there is one way to save.

    - **Rename** opens a one-field dialog, "Rename connection", the name prefilled and
      selected, with Rename and Cancel. It refuses a collision or an illegal name with the
      sentences in decisions 8 and 11. Focus returns to the renamed row.
    - **Forget** asks once: "Forget X? It is removed from this list. Nothing on the computer
      it connected to changes." Forget and Cancel. It is the one thing here nobody can undo.
      Focus lands on the row that follows, or on New connection when the list is empty.
    - **New connection** closes this dialog and opens the New connection dialog. Cancel
      from there returns to the window, not to this dialog: dialogs do not stack.

16. **With nothing saved, the dialog still opens**, with the list replaced by the sentence
    "No saved connections yet. New connection starts one, and Acter offers to save it once
    it is up." and focus on New connection. One place to learn, and the empty list says what
    to do about itself, which is the unconnected window's own rule.

    The unconnected window's Connect button, the ended session's Connect button and macOS's
    Cmd+K all open this dialog.

### The New connection dialog

17. **Today's Connect dialog, renamed New connection, and otherwise unchanged.** Kinds list,
    the shared panel, the set-up checkbox with its Help button, Connect and Cancel. No name
    field: naming happens after the connection is up. The panel is extracted from
    `connect_dialog.ts` into a component both dialogs own, taking a `ProfileId` and the two
    settings in and giving them back; the kinds list and the names list are each an
    `OptionList` as today.

### Saving: offered once, and from the File menu

18. **File → Save connection opens the Save connection dialog.** Title "Save connection".
    One text field, "Name", prefilled with the session's origin when it has one and
    otherwise with a suggestion: "marlon at example.org" for SSH, the distribution name for
    WSL, the edition's label for PowerShell, "Command Prompt" for cmd, the scenario name for
    a scripted session. Buttons Save and Cancel; Escape cancels. Focus lands on the field
    with its text selected, so typing replaces the suggestion. Save calls
    `save_connection`, announces its sentence, and returns focus to where it was. A
    refusal keeps the dialog open with the sentence announced and focus back in the field.

    Unconnected, the item does not open a dialog; it announces "Nothing is connected, so
    there is nothing to save." in the same register as every other unconnected sentence.

19. **After a new connection comes up, the same dialog is offered, once.** The moment is
    after `connectTo` resolves, which is after the set-up dialog has been answered where it
    applies, and before the connection sentence is announced. It opens only when all of
    these hold: the session has no origin, the preference below is not set, and the far
    end is available to save (a release build never offers to save a scripted session,
    because it could not list one).

    In this offering shape the dialog carries two more things: a paragraph, "You can also
    save later from the File menu, under Save connection.", and a checkbox, "Do not offer
    to save new connections", unticked. The buttons read Save and Not now. Ticking the
    checkbox and pressing either button writes `offer_to_save=no` to `preferences`; the
    File menu item is unaffected by it, which the paragraph is there to say. The preference
    sits behind a `Preferences` driven port with a fake, like `Explained`.

    **The connection sentence is announced when focus lands in the edit field afterwards**,
    exactly as 13.3 moved it, followed by who has the keys, and then, when a save happened,
    "Saved as X." The connection is the news, the keys are what the next keypress needs, the
    save is a receipt.

### Launch

20. **`acter --connect <name>` is asked for by the backend and carried out by the
    frontend.** The composition root parses the name and answers it from
    `requested_at_launch` as `Connect { name }`, or as `Unknown { name, said }` when no saved
    connection has that name, with `said` reading "There is no saved connection named X."
    The frontend, on startup, calls `connectTo` for a `Connect` exactly as if the user had
    chosen the row, so a saved SSH connection asks its host-key and password questions in
    the window like any other; for an `Unknown` it opens unconnected and announces `said`.
    Nothing is spawned before there is a window to ask in, which is what B9 already
    requires of every SSH attempt.

### The Windows installer and the portable package

21. **The Windows installer is per user and never asks for administrator rights.** Tauri's
    NSIS target with `installMode` set to `currentUser`, installing under
    `%LOCALAPPDATA%`. `bundle.active` becomes true for Windows. The portable package is a
    zip built by CI holding `acter.exe` and nothing else, built with
    `--features portable`, which is what makes decision 3 true. **Two builds, and that is the
    cost of decision 3's amendment**: the installer's binary and the zip's binary are not the
    same file, so CI runs the build twice and, when there is a certificate, signs both.
    Neither package is signed by this entry; a SmartScreen warning is a known cost until a
    certificate exists, and it is said in the README rather than worked around.

### Menus and help

22. **File gains two items on both platforms: New connection and Save connection.** Windows:
    Connect, New connection, Save connection, Exit, in that order, reached through F10 or
    Alt as today, with no new global shortcut, because a shortcut is a keystroke-map decision
    for DESIGN and not this spec's. macOS: the same three under File with Cmd+K for Connect
    as today, Cmd+N for New connection and Cmd+S for Save connection, the platform's own
    spellings for new and save. `menuActions` gains `newConnection` and `saveConnection`,
    and both the document menu bar and the system menu are handed the same object, so
    neither platform can drift (M3, decision 5).

23. **The help's "Connecting to a shell" section is rewritten** to describe the two dialogs,
    the offer to save, File → Save connection, that edits in the panel are for this attempt
    only, that a password is never saved, and where the settings folder is. Plain
    sentences, in the register the section already has.

### What does not change

24. Passwords and passphrases: B9 decisions 4 and 5 stand unchanged. The password dialog
    is asked at connect time and the panel can never hold one.
25. `known_hosts` handling, `ExplainedShells`, the set-up dialog, the connecting dialog, the
    host-key and unverified dialogs: untouched except for the folder they live in.
26. Phase 1 still runs one session at a time; connecting still replaces the session.

## Files touched

Four PRs, in this order, each short enough to be judged on its own. The spec file lands in
the first; the later ones amend it in place if implementation forces a change.

**PR 26.1, the settings folder and the launch switch** (Rust, plus one About line)

- `crates/acter-app/src/container.rs`: `settings_directory()` replacing
  `profiles_directory()`, `records_directory` taking the executable directory and the
  packaging, `ACTER_SETTINGS_DIR`, `--connect` parsed into a `LaunchRequest`.
- `crates/acter-app/Cargo.toml`: the `portable` feature decision 3 keys on. It is declared
  in 26.1 because 26.1 is what reads it; the CI job that builds with it is 26.4's.
- `crates/acter-core/src/entities/protocol_commands.rs`: `LaunchRequest`.
- `crates/acter-core/src/ports/driving/connect_api.rs`: `requested_at_launch`.
- The About dialog's settings-folder line: `ui/src/adapters/about_dialog.ts`,
  `ui/src/views/main_window.html`, and the invoke that answers the path.
- `docs/specs/26-connection-manager.md`, this file; `docs/specs/b8-profile-store.md`
  gains its superseded note; `docs/DESIGN.md` and `docs/ROADMAP.md` as below.

**PR 26.2, the connection store** (Rust only)

- `crates/acter-core/src/entities/saved_connection.rs`: `SavedConnection`, its target
  enum, its serde shape and version, the name rule, and the mapping to and from
  `ProfileId`.
- `crates/acter-core/src/ports/driven/connection_store.rs` and
  `crates/acter-core/src/ports/driven/preferences.rs`: the ports and their fakes.
- `crates/acter-app/src/adapters/connection_files.rs` and `preferences_file.rs`: the
  filesystem adapters.
- `crates/acter-core/src/services/connect.rs`: `saved`, `save_connection`,
  `rename_connection`, `forget_connection`, `origin` on `use_profile`, `saved_as` and
  `line_owner` on `Connected`, availability judged against discovery.
- `crates/acter-app/src/routers/connect.rs` and `ui/src/protocol.ts`: the new invokes and
  the regenerated types.

**PR 26.3, the two dialogs, the save dialog and the menu** (frontend)

- `ui/src/adapters/connection_panel.ts`: the shared panel, extracted.
- `ui/src/adapters/connect_dialog.ts`: the saved-connections dialog.
- `ui/src/adapters/new_connection_dialog.ts`: today's dialog, renamed.
- `ui/src/adapters/save_connection_dialog.ts`, `rename_connection_dialog.ts`, and the
  Forget confirmation.
- `ui/src/adapters/menu_bar.ts`, `system_menu.ts`, and `crates/acter-app`'s native menu:
  the two new items.
- `ui/src/controllers/app.ts`: the offer after connecting, the launch request, the three
  sentences in order.
- `ui/src/ports/connect_api.ts`, `ui/src/routers/tauri.ts`, `ui/src/main.ts`,
  `ui/src/views/main_window.html`: wiring, markup, help text.
- `ui/test/adapters/*.test.ts` for every dialog, `ui/test/controllers/app.test.ts`, and
  `e2e/test` for the saved-connection flow against a fixture `ACTER_SETTINGS_DIR` holding
  one saved scripted connection.

**PR 26.4, the installer and the portable package**

- `crates/acter-app/tauri.conf.json`: `bundle` for Windows, NSIS, `currentUser`.
- `.github/workflows`: the installer and the portable zip as build artifacts.
- `README.md`: how to install, how to run portable, and the SmartScreen sentence.

**DESIGN.md amendments, landing with PR 26.1**

- "Configuration: profiles" becomes "Saved connections": what one holds, that a password
  is never in it, that edits before connecting are for that attempt.
- "Connect is a dialog: a kind, then what that kind needs": the paragraph "Saving comes
  after it works, not before" is replaced by the two-dialog shape and the once-offered save.
- New: "Where Acter keeps its settings", decisions 2 to 6 in DESIGN's register.
- "Acter starts unconnected, and `--profile` is the only switch" becomes `--connect`.
- The Windows and macOS menu lists gain the two items.

**ROADMAP.md**: entry 26 rewritten to point here, with 26.1 to 26.4 as sub-entries, each
flipped to Done by the PR that lands it; 28.8 closes by reference to decision 11.

## Definition of done

1. `settings_directory()` answers portable when the build was packaged portable, installed
   otherwise, the variable over both, and is tested for both platforms and both packagings as
   a pure function. `known_hosts` and `explained_shells` are read from and written to it.
2. `acter --connect <name>` starts that saved connection through the window, asking its
   questions there; an unknown name opens unconnected and says so.
3. A saved connection round-trips through the filesystem adapter for every kind in
   decision 7, and a malformed file is named with a speakable reason while the rest load.
4. `save_connection` replaces its origin, refuses another name that exists, refuses an
   illegal name, and records who owns the line at the moment of saving.
5. A saved PowerShell connection survives its edition moving to a different path, and a
   saved distribution that is gone is listed as not available with instructions.
6. The Connect dialog lists names, loads the panel on arrow, announces the one-line
   summary, and its Connect uses the panel's current values without writing.
7. The offer to save appears once after a new connection, never after a saved one, never
   when the preference is set, and its checkbox writes the preference.
8. Rename and Forget do what they say and put focus where decision 15 says.
9. The About dialog says the settings folder and whether Acter is portable or installed.
10. The Windows installer installs without elevation and keeps its settings with the
    account's application data; the portable zip runs and writes into a `settings` folder
    beside itself.
11. `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests, `npm -w acter-ui
    test`, `npm run typecheck` and the end-to-end suite green in every PR.
12. The checklist below is run with NVDA on Windows in PR 26.3, its results in the PR body
    one line per item, naming the reader version and the capture mode, and saying which
    items were agent-observed.

## Manual checklist (Windows, NVDA)

Run against a fixture `ACTER_SETTINGS_DIR` holding one saved scripted connection and one
saved SSH connection to the `docker/ssh` rig.

- [ ] File → Connect opens the dialog, and the first thing said is a saved connection's
      name, not the dialog's title.
- [ ] Arrowing between the two names announces each panel's one-line summary without
      moving focus.
- [ ] Tab from the list lands in the panel's first control, which holds the saved value.
- [ ] Changing the SSH port, pressing Connect, then reopening the dialog shows the saved
      port unchanged.
- [ ] File → Save connection on that session says "Saved as" and the name, and the
      reopened dialog shows the new port.
- [ ] File → New connection opens today's dialog under its new title, and connecting to the
      scripted session offers to save; Not now leaves nothing saved and the connection
      sentence is heard after the dialog closes.
- [ ] Saving from the offer with the suggested name adds a row, and "Saved as" is heard
      after the connection sentence and the keys sentence, in that order.
- [ ] Ticking "Do not offer to save new connections" stops the offer on the next new
      connection, and File → Save connection still works.
- [ ] Rename refuses the other row's name with the sentence, and accepts a new one, with
      focus back on the renamed row.
- [ ] Forget asks once, removes the row, and focus lands on the next row or on New
      connection.
- [ ] With the fixture emptied, the dialog says there are no saved connections and focus is
      on New connection.
- [ ] File → Save connection while unconnected announces that nothing is connected.
- [ ] Help → About reads the settings folder path and whether Acter is portable.
- [ ] Human-only: nothing in any of the above played a sound that was not expected.
