# Spec: PR 26 — the connection manager, and where Acter keeps its settings

Connections get names and are saved. The Connect dialog becomes a list of those names, each
loading its own properties into a panel a user can change before connecting. A new
connection is made in a dialog of its own, and once it is up Acter offers to save it, once.
Nothing here ever saves a password. Underneath, everything Acter writes moves into one
settings folder holding one JSON document, and where that folder is follows from how this
copy of Acter was packaged: the folder Acter was started from for a development build,
beside the program for a portable one, and the account's own configuration folder for an
installed one.

Agreed in conversation 2026-09-12. It supersedes
[b8-profile-store.md](b8-profile-store.md), which was agreed 2026-08-23 and never
implemented; that file stays, with a note at its top pointing here, because the reasoning it
records for JSON, for a store the environment can point elsewhere, and for a failure that is
named rather than swallowed still holds and is reused below. What did not survive is one file
per connection, for the reason decision 7 gives.

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
- Three crates this spec needs are already compiled into the product through Tauri: `dirs`,
  which asks the operating system where an account's configuration lives rather than reading
  an environment variable; `tempfile`; and `serde_json`, which is a direct dependency
  already. The only new one anywhere below is `vergen-gitcl`, and it is a build dependency.
- The repository has no tags, and CI checks out with `actions/checkout@v4` and no
  `fetch-depth`. Both matter to decision 3 and are answered in decision 21.

## Design decisions

### The vocabulary is saved connections, and the folder is settings

1. **The word is connection.** The product's own dialog is called Connect and the thing a
   user names is what they connect to. "Profile" survives only inside the code, as
   `ProfileId`, which is not renamed here because it is a type on the wire and this spec has
   enough surface. `--profile` becomes `--connect <name>`. `ACTER_PROFILES_DIR` becomes
   `ACTER_SETTINGS_DIR`; nothing shipped reads the old name, so there is no alias.

### Where Acter keeps its settings

2. **One settings folder, and one JSON document in it.** Everything Acter decides on a
   person's behalf lives in `settings.json`:

   ```json
   { "format": 1, "connections": [] }
   ```

   `format` is there from the first document written. Every setting this product grows is a
   new field with a serde default, so a document written by an older Acter still loads and
   nothing needs migrating for a while. **Nothing in it is a free-text key**: the document is
   a typed structure, which is what lets the compiler find every reader of a setting whose
   shape changes.

   Beside it in the same folder:
   - `settings-previous.json`, the document as it was before the last successful write
     (decision 9);
   - `settings-unreadable.json`, which exists only when something went wrong (decision 9);
   - `known_hosts`, Acter's own record of accepted host keys, unchanged in format;
   - `explained_shells`, unchanged, still one shell name per line.

   **`explained_shells` is deliberately not folded in.** It is a set rather than a scalar, it
   works, and moving it would be a format change with nothing user-visible to show for it. It
   changes folder and nothing else. Folding it in is its own entry if anybody ever wants it.

3. **How this copy was packaged is decided when it is built, and never guessed at when it
   runs.** Three packagings, and the settings folder follows from them:

   - **Development**, which is any debug build. The folder is `settings` in the directory
     Acter was started from. A developer needs no variable set to get a sane answer, which is
     the whole reason this case exists.
   - **Portable**, a release built with the `portable` Cargo feature on `acter-app`. The
     folder is `settings` beside the program, and on macOS beside the `.app` bundle rather
     than inside it, because a file written inside the bundle breaks its signature the moment
     M4 signs it. A macOS build that is not in a bundle is beside its own executable like
     every other platform, because there is no bundle to be outside of.
   - **Installed**, a release without the feature, which is what the installer ships. The
     folder is `settings` under the directory this operating system keeps an account's
     configuration in, which is `%APPDATA%\acter` on Windows and
     `~/Library/Application Support/acter` on macOS.

   The feature wins over the debug default, so a portable build can be made and driven on a
   developer's machine.

   **Where the account's directory is comes from `dirs`, not from an environment variable.**
   On Windows `dirs::config_dir()` asks the known-folder API, where reading `%APPDATA%`
   trusts a variable that can be missing or redirected. It is already compiled into the
   product through Tauri, and it collapses the per-platform branch in the installed case to
   nothing: both platforms answer their own base and both then join the same suffix.

   **Decided in conversation on 2026-09-12, replacing this decision's first version**, which
   said the *presence* of a folder named `settings` beside the program made a copy portable.
   That inferred a fact about the package from a side effect on disk. A folder with that name
   is something plenty of directories happen to contain, so a copy run from one of them would
   have silently read and written that store instead of the person's own, and because the
   marker was also the storage their saved connections would simply appear to be gone. For
   somebody who cannot glance at a folder to check, that is the wrong failure to trade for the
   convenience of converting a copy by hand. It also left development with no answer of its
   own, which is why the three cases above exist rather than two.

   **Acter creates the folder on the first write, not at startup**, whichever packaging it is,
   so a machine Acter was only ever run on and never saved anything from has no folder.

   **`ACTER_SETTINGS_DIR` wins over the packaging**, exactly as `ACTER_PROFILES_DIR` did: it
   is what points the suites and the NVDA fixture at a directory made for them, and a manual
   pass whose saved connections depend on this machine's history is not repeatable. It is
   also the one way to keep settings somewhere the packaging did not choose, which is what
   converting a copy by hand means now. An empty value is ignored rather than treated as a
   path.

   **The version is a build-time fact too, and it joins the packaging.** `CARGO_PKG_VERSION`
   says `0.1.0` and tells nobody what is running. `acter-app` already has a build script, and
   it stamps the real version there through `vergen-gitcl`.

   **The crate rather than hand-written git calls**, decided 2026-09-12 after first choosing
   the other way. Asking git for a tag and a commit is three lines; what is not three lines
   is everything around it, and `vergen-gitcl` has already done all of it: it emits the
   `rerun-if-changed` instructions for the git head and for the ref that head points at, it
   does not fail a build in a directory that is not a repository, and **every value it emits
   can be overridden by an environment variable of the same name**, which is what a release
   needs. It shells out to git rather than linking a git library, so it costs a build
   dependency and no runtime one.

   **What is taken from it**: the describe string and the short commit. Rust reads them with
   `option_env!`, and **a pure function turns them into the two things a person meets**: the
   identifier a bug report carries, and the sentence About speaks.

   **A release is named by its tag, and the tag names a platform.** Tags are
   `<platform>-vx.y.z`, so `windows-v1.0.0` and `macos-v1.0.0` are the same release of the
   same product for two machines. **The version is the numeric triple and nothing else**: a
   listener hears "Version 1.0.0" on either platform, because the platform is a fact about
   which file they downloaded rather than about which Acter they are running.

   So the function's rule, in order:
   - a describe string shaped `<platform>-vx.y.z`, with all three numbers numeric, is a
     release, and the version is `x.y.z`;
   - anything else with a commit behind it is `development-<short commit>`;
   - nothing at all is `CARGO_PKG_VERSION`, which is what a source tarball with no git has.

   A tag that does not match the shape is not a release. It falls to the development case
   rather than being read out as a version, because a malformed tag claiming to be `1.0` is
   worse than one saying it is a development build. The function does not check that the
   platform in the tag is the platform being built: which tag builds which artifact is the
   workflow's business (decision 21), not the binary's.

   It is a function rather than something the build script prints, because these strings are
   read aloud and a build script's output cannot be unit tested. Its tests are the three rules
   above plus a malformed tag.

4. **The rule is one pure function, and it is tested for both platforms and all three
   packagings without being any of them.** It takes the operating system, the packaging, the
   account's configuration directory and the directory the program is in, and answers a
   folder together with how it got there. The composition root is the only caller that reads
   the real answers.

   **The packaging reaches it as a value, from one `cfg!` expression**, so both arms are
   compiled and an ordinary `cargo test` covers every branch. A `#[cfg]` on the rule itself
   would compile half of it out of every test run, which is the trap ARCHITECTURE's
   platform-divergence rule names and the reason M1 exists.

   Its tests cover: development, portable on Windows, portable on macOS staying outside the
   bundle, a macOS build that is not in a bundle, installed on both platforms, the variable
   winning over the packaging, a machine that reports no configuration directory at all, and
   the build's own packaging matching its feature.

5. **The settings folder and the version are said where a user can read them.** The About
   dialog gains one line, "Settings folder:" followed by the path and then a whole sentence
   saying how Acter came to be using it: running portable, installed, a development build, or
   told where by the variable. This is the cheapest answer to "where did that go", and the
   About dialog is already a readable document.

   The version line says the stamped version rather than the Cargo one, and it says it in
   words. `development-521c956` is a value, not a sentence: a listener hears "Development
   build, commit 521c956", and on a release "Version 1.0.0". The raw identifier is what the
   object holds, so a bug report can still carry it, and the About dialog is copyable text —
   nothing tries to spell a commit out loud.

   Everything About reads comes out of the settings object through managed state. The dialog
   asks the environment nothing, because a second lookup could name a folder the files are
   not in.

6. **A write that fails is a sentence, never a silent nothing.** "Could not save the
   connection: the settings folder C:\path is not writable." Naming the folder is the whole
   point; a listener with an unwritable Program Files cannot see a red squiggle.

### What a saved connection is

7. **A name, a target, and the two settings the session has.** One entry in the document's
   `connections` array, tagged by kind and carrying only that kind's facts:

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
   implementation, and B8's reason stands: settings nothing reads are settings nothing tests.

   **One array rather than one file per connection, decided in conversation on 2026-09-12.**
   B8 wanted a file each so a person could copy one, mail one, delete one and diff two, and
   so that a damaged file cost only its own connection. That is given up for one document
   under one lock with one write path, which is what makes a single settings object possible
   at all: two stores are two things that can disagree about whether a write happened.
   Decision 9 is what pays for the loss.

8. **A name is something a listener hears, and the rule is said in one sentence.** A name
   cannot contain the characters slash, backslash, colon, star, question mark, double quote,
   less than, greater than or bar; it cannot be empty or only spaces; and it is compared
   case-insensitively. A name that breaks the rule is refused at Save with the sentence "A
   name cannot contain slash, backslash, colon, star, question mark, quote, less than,
   greater than or bar." Spelled out in words, because a screen reader reads the characters
   themselves unreliably.

   **The reason changed with decision 7 and the rule did not.** It used to be a filesystem
   rule, because the name was a file name. Now the reason is the one that was always the
   better one: every name here is read aloud, and a name full of punctuation is read aloud
   badly. Case-insensitive for the same reason rather than for Windows' — two connections
   whose names differ only in case are two rows a listener cannot tell apart.

9. **A document that cannot be read is moved aside, named, and never written over.** One
   file holds everything now, so B8's per-file failure has no meaning and its safeguard is
   replaced by three rules that matter more:

   - **Every write is atomic.** The document goes to a temporary file in the same folder and
     is renamed over the target. A crash, a full disk or a killed process cannot leave a
     half-written document, which with one file would mean every saved connection at once.
   - **The previous document is kept.** Each successful write leaves the version it replaced
     as `settings-previous.json`, so a bad write is recoverable by hand and a person can be
     told which file to rename.
   - **A document that will not parse is moved aside at load**, to `settings-unreadable.json`,
     and Acter starts with nothing saved. Without this the first save after a corruption
     destroys the evidence silently. The Connect dialog says so where it would otherwise say
     the list is empty (decision 16), in a sentence naming both files and what was wrong with
     the first, in words.

   A settings folder that does not exist yet, or a document that is not there, means no saved
   connections and no error at all. That is an ordinary first run.

### The backend: one settings object behind ports

10. **One object owns the folder, the document and the lock.** It is built first in the
    composition root, before the connect service, and everything downstream is handed what it
    needs from it rather than resolving a path of its own. The record of host keys and the
    record of explained shells take their folder from it, and so does the connection store.

    **Where it lives.** The port, the entities and the fake are in `acter-core`; the object
    that touches the file is an adapter in `acter-app/src/adapters/`. Core keeps its zero-I/O
    rule, and this is where `ExplainedShells` already does the same thing one size smaller. No
    new crate: a crate earns its place by isolating a heavy or platform-bound dependency, the
    way `acter-transports`, `acter-term` and `acter-shells` each do, and this isolates
    `serde_json`, which core already uses. If it grows past a few hundred lines it can be
    carved out later without anything above it changing.

    **Two halves, and they behave differently.**

    - **The runtime values are set in the constructor and have getters and no setters**: the
      packaging, the settings folder with its standing, and the version. They are not in the
      document and are never written.
    - **The document is behind a `std::sync::RwLock`**, matching the idiom `ConnectService`
      already uses, with typed getters and setters and no way to reach the lock from outside.
      Every setter takes the lock, changes the document, writes it, and answers a whole spoken
      sentence when the write fails.

    **`ConnectionStore` is the driven port the connect service sees**, with a fake for service
    tests and this object as its only real implementation:

    ```rust
    pub trait ConnectionStore: Send + Sync {
        /// Everything saved, and the sentence to say instead when the document was
        /// unreadable (decision 9).
        fn saved(&self) -> SavedConnections;
        fn save(&self, connection: SavedConnection) -> Result<(), String>;
        fn rename(&self, from: &str, to: &str) -> Result<(), String>;
        fn forget(&self, name: &str) -> Result<(), String>;
    }
    ```

    The connect service depends on this and never sees the settings object, which is what
    keeps a service depending on the port it actually uses. The About router holds the object
    itself, through managed state, because the runtime values are the only thing it wants.
    Because both halves are one object there is one lock and one write path, and nothing has
    to ask anything else to persist on its behalf.

    **The crates, decided after looking** (2026-09-12), and recorded so it is not
    relitigated. For the store itself: `serde_json`, `tempfile` and the standard library's
    lock, all already compiled here.
    `rustbreak` comes closest to doing the whole job, with a single file, an internal lock and
    atomic saves, but its serializers are RON, YAML and bincode and it has no JSON.
    `persister` has JSON, atomic writes and auto-save but no lock at all, and is at version
    0.2. `tauri-plugin-store` is the official one and its file is exactly this shape, but it
    saves with a plain `fs::write` over the target, which is the one failure this document
    cannot afford, and it needs an `AppHandle`, which would move construction into Tauri's
    setup. `confy` has no JSON. `config` and `figment` are read-only loaders and the wrong
    shape. No accessor-derive crate either: `getset` is good and would generate three trivial
    runtime getters, two of them with a return type this codebase would not choose, and by its
    own documentation it is not for accessors that carry logic, which every setter here does.

11. **`ConnectApi` grows, and `use_profile` learns where a session came from.**

    **Nothing crosses the IPC boundary as a key and a value.** The frontend never reads or
    writes the settings document; it calls a named action and gets a typed answer, exactly as
    it already does for `connectable`, `connected` and `about`. A stringly surface would give
    up the one guarantee that makes a new setting a compile error rather than a silent
    nothing, and it would let the frontend write keys with domain invariants behind them —
    the name rule and the document's format among them — at which point the backend can no
    longer promise what is in its own file. A named action is also where the spoken sentence
    lives. The cost, stated: a setting the frontend reads costs one invoke, and two if it is
    written there as well.

    - `saved(&self) -> SavedConnections`: every saved connection, freshly read on every call
      for `connectable`'s reason (B7, decision 6), plus the sentence from decision 9 when the
      document could not be read. A row carries the name, the `ProfileId` the panel is loaded
      from, `set_up`, `line_owner`, `available`, and `instructions` when it is not available.
      Availability is judged against what discovery answers now: a distribution that was
      uninstalled, an edition that is gone, a scripted scenario in a release build.
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

    **This is also where a document that would not parse is reported** (decision 9). The
    sentence takes the place of the empty-list one, names both files and says what was wrong
    with the first, in words, and focus still lands on New connection: there is nothing saved
    either way, and the difference is whether the person should go looking for a file.

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
    checkbox and pressing either button records the preference; the File menu item is
    unaffected by it, which the paragraph is there to say.

    **The preference is a field in the settings document** (decision 2), not a file or a port
    of its own — the separate `Preferences` port this spec first described is gone, because
    the settings object is what it would have been. The frontend reaches it through two named
    invokes and no others, per decision 11: one asking whether to offer, called at the moment
    of the offer, and one recording that the box was ticked.

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
    zip built by CI holding `acter.exe` and nothing else, from a second build carrying the
    `portable` feature, which is what makes decision 3 true. **Two builds is the cost of
    decision 3**: the installer's binary and the zip's binary are not the same file, so CI
    compiles twice and, when there is a certificate, signs both.
    Neither package is signed by this entry; a SmartScreen warning is a known cost until a
    certificate exists, and it is said in the README rather than worked around.

    **A release is built when two things are true, and the workflow is what knows them**: the
    commit is on the main branch, and it carries a tag shaped `<platform>-vx.y.z`. Nothing
    else produces a release, and the binary is not asked to work any of this out.

    Three things the workflow therefore does, and each is there for a measured reason:
    - **Check out with `fetch-depth: 0`.** The default is a shallow clone with no tags, so
      today nothing in CI could see a tag at all, and the ancestry check below needs history.
    - **Prove the tagged commit is on main**, rather than trusting that tags are only ever
      made there.
    - **Say which tag this build is for**, by setting the describe variable decision 3's
      crate reads from `GITHUB_REF_NAME`. **This is required, not belt and braces**: with a
      tag per platform, `windows-v1.0.0` and `macos-v1.0.0` sit on the same commit, and
      `git describe` would pick one of them by its own rules rather than the one being
      built.

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

**One pull request, decided in conversation on 2026-09-12.** This spec was first laid out
as five, each short enough to judge on its own, which is what CLAUDE.md's short-PR rule asks
for. The user chose one instead: the whole entry lands together or not at all. The rule is
overridden here deliberately rather than forgotten, and the reason it is safe to override is
that the pieces are useless apart — a settings object nothing reads, a store no dialog
reaches, or dialogs with nothing behind them are each harder to judge than the whole.

The spec file lands in this pull request, and any change implementation forces is written
into it here, under an "Amended in implementation" heading.

The files, grouped by what they are rather than by when they land.

**The settings object** (Rust, plus the About line)

- `crates/acter-core/src/entities/stored_settings.rs`: the document, its `format`, its
  `connections`, and its serde shape with defaults.
- `crates/acter-core/src/entities/saved_connection.rs`: `SavedConnection`, its target enum,
  the name rule of decision 8, and the mapping to and from `ProfileId`.
- `crates/acter-core/src/ports/driven/connection_store.rs`: the port of decision 10,
  `SavedConnections`, and the fake.
- `crates/acter-app/src/adapters/settings_file.rs`: the settings object. The runtime values,
  the document under the lock, the atomic write, the previous copy, and the unreadable
  document moved aside.
- `crates/acter-app/src/container.rs`: the packaging, the pure folder rule, the version, and
  building the object first. `profiles_directory` and `ACTER_PROFILES_DIR` go.
- `crates/acter-app/build.rs`: the version stamp, through `vergen-gitcl`.
- `crates/acter-app/Cargo.toml`: the `portable` feature decision 3 keys on, `dirs` and
  `tempfile` as direct dependencies, and `vergen-gitcl` as a build dependency. The CI job
  that builds with the feature, and the one that makes a tag reachable, are below.
- `crates/acter-app/src/routers/about.rs`: the folder, the standing and the version.
- `ui/src/adapters/about_dialog.ts`, `ui/src/ports/app_shell.ts`,
  `ui/src/views/main_window.html` and their test: the two new lines.
- `docs/specs/26-connection-manager.md`, this file; `docs/specs/b8-profile-store.md` gains
  its superseded note; `docs/DESIGN.md` and `docs/ROADMAP.md` as below.

**The launch switch** (Rust, plus the startup call)

- `crates/acter-core/src/entities/protocol_commands.rs`: `LaunchRequest`.
- `crates/acter-core/src/ports/driving/connect_api.rs`: `requested_at_launch`, answered
  against the store so an unknown name is `Unknown` rather than an attempt.
- `crates/acter-app/src/container.rs`: `--connect` parsed, in both spellings.
- `crates/acter-app/src/routers/connect.rs`, `ui/src/protocol.ts`, `ui/src/controllers/app.ts`:
  the invoke, the regenerated types, and the window carrying the request out.

**The connection actions** (Rust only)

- `crates/acter-core/src/services/connect.rs`: `saved`, `save_connection`,
  `rename_connection`, `forget_connection`, `origin` on `use_profile`, `saved_as` and
  `line_owner` on `Connected`, availability judged against discovery.
- `crates/acter-app/src/routers/connect.rs` and `ui/src/protocol.ts`: the new invokes and
  the regenerated types.

**The two dialogs, the save dialog and the menu** (frontend)

- `ui/src/adapters/connection_panel.ts`: the shared panel, extracted.
- `ui/src/adapters/connect_dialog.ts`: the saved-connections dialog.
- `ui/src/adapters/new_connection_dialog.ts`: today's dialog, renamed.
- `ui/src/adapters/save_connection_dialog.ts`, `rename_connection_dialog.ts`, and the
  Forget confirmation.
- `ui/src/adapters/menu_bar.ts`, `system_menu.ts`, and `crates/acter-app`'s native menu:
  the two new items.
- `ui/src/controllers/app.ts`: the offer after connecting, the two named invokes of
  decision 19, and the three sentences in order.
- `ui/src/ports/connect_api.ts`, `ui/src/routers/tauri.ts`, `ui/src/main.ts`,
  `ui/src/views/main_window.html`: wiring, markup, help text.
- `ui/test/adapters/*.test.ts` for every dialog, `ui/test/controllers/app.test.ts`, and
  `e2e/test` for the saved-connection flow against a fixture `ACTER_SETTINGS_DIR` holding a
  document with one saved scripted connection.

**The installer and the portable package**

- `crates/acter-app/tauri.conf.json`: `bundle` for Windows, NSIS, `currentUser`.
- `.github/workflows`: the installer, and the portable zip from a second build carrying the
  `portable` feature.
- `README.md`: how to install, how to run portable, and the SmartScreen sentence.

**DESIGN.md amendments**

- "Configuration: profiles" becomes "Saved connections": what one holds, that a password
  is never in it, that edits before connecting are for that attempt.
- "Connect is a dialog: a kind, then what that kind needs": the paragraph "Saving comes
  after it works, not before" is replaced by the two-dialog shape and the once-offered save.
- New: "Where Acter keeps its settings", decisions 2 to 6 in DESIGN's register, including
  that the packaging decides and that a development build keeps its settings where it was
  started from.
- "Acter starts unconnected, and `--profile` is the only switch" becomes `--connect`.
- The Windows and macOS menu lists gain the two items.

**ROADMAP.md**: entry 26 rewritten to point here and flipped to Done by this pull request,
in the style of the entries around it — what changed, the PR number, what was measured. No
sub-entries, because there is one PR. Entry 28.8 closes by reference to decision 11, since
who holds the keys is recorded at the moment of saving.

## Definition of done

1. The settings folder follows the packaging: the folder Acter was started from for a
   development build, beside the program for a portable one, the account's configuration
   folder for an installed one, and whatever `ACTER_SETTINGS_DIR` names over all three. The
   rule is a pure function tested for both platforms and all three packagings.
   `known_hosts` and `explained_shells` are read from and written to that folder.
2. `acter --connect <name>` starts that saved connection through the window, asking its
   questions there; an unknown name opens unconnected and says so.
3. A saved connection round-trips through the settings object for every kind in decision 7.
   A write goes to a temporary file and is renamed, the document it replaced is kept, and a
   document that will not parse is moved aside and named with a speakable reason rather than
   written over.
4. `save_connection` replaces its origin, refuses another name that exists, refuses an
   illegal name, and records who owns the line at the moment of saving.
5. A saved PowerShell connection survives its edition moving to a different path, and a
   saved distribution that is gone is listed as not available with instructions.
6. The Connect dialog lists names, loads the panel on arrow, announces the one-line
   summary, and its Connect uses the panel's current values without writing.
7. The offer to save appears once after a new connection, never after a saved one, never
   when the preference is set, and its checkbox writes the preference.
8. Rename and Forget do what they say and put focus where decision 15 says.
9. The About dialog says the settings folder, how Acter came to be using it, and the
   stamped version, each as something a listener can hear.
10. The Windows installer installs without elevation and keeps its settings with the
    account's configuration; the portable zip, built with the feature, runs and writes into a
    `settings` folder beside itself.
11. `cargo fmt`, `cargo clippy --workspace --all-targets`, the same clippy with
    `--features portable`, workspace tests, the workspace tests again with
    `--features portable`, `npm -w acter-ui test`, `npm run typecheck` and the end-to-end
    suite, all green.
12. The checklist below is run with NVDA on Windows, its results in the PR body one line
    per item, naming the reader version and the capture mode, and saying which items were
    agent-observed.

## Amended in implementation

The spec file lands in the pull request that implements it, and any change the
implementation forced is written here rather than left in a commit message.

**A. Acter's own record of host keys is in the settings document, not a file beside it.**
Decision 2 said `known_hosts` stayed as it was, "unchanged in format". Asked for by the
user on 2026-09-12, on reading the implementation: if the document is where everything
Acter decides on a person's behalf lives, a record of which servers they told it to trust
belongs in it. It is a typed list under `host_keys`, and each record is the host, the port,
the kind of key, the **fingerprint** as `ssh-keygen -l` prints it, and the **day** it was
accepted.

The fingerprint rather than the key, because a base64 key is sixty-eight characters of
noise and the fingerprint is what a provider printed, what a colleague read out, and what
the dialog put in front of the user when they accepted it. Comparing a server's offer
against it is the same operation either way. The algorithm is kept beside it because two
behaviours need it: a key recorded under a *different* algorithm is an unknown key rather
than a changed one, and the algorithms already on file are what Acter offers a server first
so a familiar host is not asked about on every second connection. The day, because a record
nobody can date cannot answer "when did I trust this?", which is the question somebody asks
when a key changes.

**B. The user's own `~/.ssh/known_hosts` is read into the same list, flagged.** Asked for
in the same conversation. Anything asking what is known about a server now gets one answer
rather than two to merge, and every row read out of that file carries
`HostKeyOrigin::Native`. **B9's decision 5 is untouched**: a native record is never written
into Acter's document, and that is structural rather than remembered — the origin is
`#[serde(skip)]`, so a record in the document is Acter's own by construction, and the port's
`accept` takes the facts rather than a record, so there is no native one to hand it.

**C. `SavedTarget::Wsl` carries an optional distribution.** Decision 7 said a saved WSL
connection remembers "the distribution name". `ProfileId::Shell { kind: Wsl }` — what
`ACTER_SHELL=wsl` produces — is a real live session that names none, and Save connection
has to be able to write down whatever is running. `None` means whatever distribution WSL
calls the default, which is deliberately not the same as Acter deciding which one that is
(spec B5.3).

**D. The wire type and the port's type have different names.** Decision 10 and decision 11
both called their answer `SavedConnections`, and they carry different facts: the port
answers what the document holds, and `ConnectApi` answers that resolved against discovery,
with the profile each panel loads from and whether this machine can start it now. The port's
is `StoredConnections`; the wire keeps the spec's name.

**E. The preference of decision 19 rides on `ConnectionStore`.** That decision deleted the
separate `Preferences` port and said the settings object is what it would have been — so the
two named actions reach it through the port the connect service already holds, rather than
through a second seam over the same object.

**F. A `SavedRow` carries the summary decision 13 describes.** The words a listener hears are
composed in the domain, like every other spoken string on this seam, rather than assembled by
the dialog from a `ProfileId`. It also uses commas where `ProfileId::label` uses a colon: that
label names a row in a list of kinds, where the colon separates a category from a member,
while this is said *after* the user's own name for the connection, as a description of it.

**G. The settings object holds no packaging.** Decision 10 listed it among the runtime values
with getters. It has exactly one consumer — the rule that decides where the folder is — and by
the time the object exists that rule has run and left its answer in the standing. A getter
nothing calls is a value nothing tests.

**H. `KnownHosts`'s aside covers only the user's file now.** It used to name either record as
one that could not be read. Acter's own can no longer fail on its own: it is part of the
document, and a document that will not parse is reported where the saved connections are, in
the sentence decision 16 specifies.

**I. A name that is empty gets its own sentence.** Decision 8 gives one sentence, for the
forbidden characters, and also forbids a name that is empty or only spaces. "A name cannot
contain slash, backslash…" answers a question that person did not ask, so an empty name is
refused with "A connection needs a name."

**J. `settings/` is in `.gitignore`, and the router tests write to a temporary folder.** Found
by this entry's own first test run: a development build keeps its settings in the folder it was
started from, and `cargo test` starts in the crate directory — so the suite wrote a real
`settings` folder into the working tree. Without the ignore, a `tauri dev` would put somebody's
actual saved connections and accepted host keys there too.

**K. The release workflow triggers on `windows-v*` only.** Decision 21 describes a tag per
platform, and the tag shape is what makes this not a gap: `macos-v1.0.0` gets a job of its own
the day there is one to run. Bundling and signing for macOS are entry 35's (M4), and a job that
built an unbundled binary and called it a release would be shipping something nobody signed.

## Manual checklist (Windows, NVDA)

Run against a fixture `ACTER_SETTINGS_DIR` holding a `settings.json` with one saved
scripted connection and one saved SSH connection to the `docker/ssh` rig.

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
- [ ] Help → About reads the settings folder path, how Acter came to be using it, and the
      version.
- [ ] Human-only: nothing in any of the above played a sound that was not expected.
