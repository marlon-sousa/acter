# B8 — the profile store, and `--profile`

Roadmap entry 26, lane 2. Agreed in conversation 2026-08-23, in the conversation that
introduced actions and profiles. Depends on 25 (B7), whose actions it feeds.

DESIGN has described profiles as Decided since the beginning — a bundle of transport, shell
adapter and settings, with a Defaults profile others inherit from. This is the entry where
they stop being a description and become files a user owns.

## Decisions

### 1. Files on disk, one per profile, JSON

`%APPDATA%\acter\profiles\<name>.json` on Windows. One file per profile, named by the
profile: a user can copy one, mail one, delete one, and diff two, and a corrupt file costs
its own profile rather than all of them.

**JSON, not TOML.** Every payload in this project is already serde JSON, the protocol types
derive it, and adding a format means adding a dependency and a second way to be wrong.
The cost is real and worth naming: JSON is unkind to hand-editing — no comments, and a
trailing comma is a syntax error a screen reader user will hear as "could not read this
profile" rather than see as a red squiggle. That is answered by decision 5, not by the
format.

Other operating systems get their locations when they get their builds. The path resolution
is one function with one Windows implementation today, so adding `$XDG_CONFIG_HOME` later is
that function's business and nobody else's.

### 2. `ACTER_PROFILES_DIR` points the store somewhere else

Development, tests, and — the reason that matters most — the **manual NVDA pass**. A
checklist item that depends on which distributions this machine has installed is not
repeatable and cannot be compared across two runs. Pointed at a fixture directory, it is
both.

It is read in the composition root, where the environment is allowed in, like every other
variable this project reads.

### 3. Stored profiles join the discovered shells; they do not replace them

`connectable()` answers with both, and this is what makes a fresh install useful with no
configuration at all: cmd, the PowerShell editions and the WSL distributions are offered
because they exist, not because someone wrote a file about them.

A stored profile is therefore about *customisation* — a starting directory, a different
auto-read threshold, a shell with arguments of the user's own — rather than about being
allowed to connect at all. A profile whose name collides with a discovered shell's label
wins, because a user who wrote a file meant it.

### 4. One switch: `acter --profile <name>`

It starts that profile's session instead of opening unconnected. It is the only argument
Acter takes.

**Arguments are parsed, never printed.** A windowed binary has no console attached — that is
the subsystem in the executable header, decided before any code runs — so a name that
resolves to nothing cannot be reported on stdout, and should not want to be: the person who
needs to know is standing in front of the window. The window opens **unconnected** and says
what was wrong with the name, using B7's unconnected state exactly as it stands.

There is deliberately **no `create` and no `list`** on the command line. Both would need
output, output needs a console, and a console needs either a second binary or
`AttachConsole` — whose known behaviour is that the shell has already printed its next
prompt before the output arrives, which reads to a screen reader as text landing on top of
whatever the user started typing. Profiles are files; creating one is editing one, and an
in-app flow arrives when it earns its place.

### 5. A profile that cannot be read is named, and the rest still load

The failure mode this design has to get right, because hand-edited JSON will be malformed
sooner or later.

- A file that does not parse **does not stop the others loading**. One bad profile is one
  bad profile.
- It is reported by name, in a sentence a listener can hear, and it says the file and what
  was wrong with it — not "invalid JSON at line 4 column 17" alone, which is unusable
  without a visual editor, and not a silent omission, which is worse: the user goes looking
  in a menu for something that is simply not there.
- An unreadable or missing directory is not an error at all. It means there are no stored
  profiles, and the discovered shells are still offered.

### 6. Behind a port, with a fake, because it touches the filesystem

```rust
pub trait ProfileStore: Send + Sync {        // ports/driven
    /// Every profile that parsed, and every file that did not, by name and reason.
    fn load(&self) -> ProfilesLoaded;
}
```

Reading a directory is I/O, so it is an adapter and it gets a port — ARCHITECTURE's
classifying question. `ConnectService` gains it beside `InstalledShells`, and both are fakes
in its tests, so the whole of "what can I connect to, including what is broken about it" is
tested with no filesystem at all.

### 7. What a profile file contains, and what it does not yet

Phase 1, deliberately small: which shell, and — where the shell allows it — a starting
directory. Everything else DESIGN lists (auto-read threshold, beep preferences, inheritance
from Defaults) has a place in the format and no implementation here, because settings that
nothing reads are settings that nothing tests.

The format carries a version field from the first file written. A store that outlives one
release without one is a migration nobody can perform.

## Files touched

- `crates/acter-core/src/ports/driven/profile_store.rs` — the port (new).
- `crates/acter-core/src/entities/profile.rs` — the profile value and its serde shape.
- `crates/acter-core/src/services/connect.rs` — `connectable()` joining stored and
  discovered.
- `crates/acter-app/src/adapters/profile_files.rs` — the filesystem implementer, and the
  path resolution including `ACTER_PROFILES_DIR`.
- `crates/acter-app/src/container.rs` — argument parsing and the launch path.

## Definition of done

- [ ] Profiles load from `%APPDATA%\acter\profiles`, and from `ACTER_PROFILES_DIR` when it
      is set.
- [ ] `connectable()` returns stored profiles and discovered shells together, with a stored
      profile winning a name collision.
- [ ] A malformed profile is reported by name with a speakable reason, and every other
      profile still loads. Tested with a fixture directory containing one broken file.
- [ ] A missing profiles directory yields no profiles and no error.
- [ ] `acter --profile <name>` starts that session; an unknown name opens an unconnected
      window that says what was wrong with the name.
- [ ] Nothing is written to disk by any of this: the store is read-only in this entry.
- [ ] The store and the parse failures are tested through the port's fake with no
      filesystem, and the filesystem adapter is tested against a temporary directory.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests clean.

## Accessibility checklist for the PR body

- [ ] Launching with `--profile` for a profile that exists connects and announces it.
- [ ] Launching with a name that does not exist announces what was wrong, and the window is
      usable — the Acter menu still connects.
- [ ] A malformed profile file is announced by name at startup rather than silently missing
      from the menu.
