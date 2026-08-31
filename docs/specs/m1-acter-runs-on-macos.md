# M1 — Acter runs on macOS, and SSH is what it offers

Roadmap entry 32, lane 3. It is the first entry of the macOS lane and the only one that is
pure repair: nothing here is a feature, and what it delivers is a build that compiles, a
suite that passes, and a window that can already connect somewhere real.

## What was true before it

Nothing about macOS had ever been run. The board said "Windows first (Linux/Mac later)" and
the architecture was written as though the port would be cheap — `portable-pty` over a Unix
pty, `russh` over any socket, `alacritty_terminal` over bytes — but no build had ever been
attempted, so that was a hope rather than a measurement.

**The hope was mostly right.** Measured 2026-08-31 on macOS 15.0 (24A335), `x86_64`,
rustc 1.98.0: every domain crate, every transport and the terminal engine compile for Darwin
untouched. What did not was small, and all of it was in the places a Windows-only project
would expect — but there were **six** of them rather than the four the board opened with,
because `acter-app` never compiled far enough for the last two to be visible.

1. **`cargo check --workspace --all-targets` failed once**: `acter_shells::WindowsTrust`
   imported without a gate in `crates/acter-transports/tests/real_session.rs`.
2. **`acter-app` did not compile at all**: `generate_context!()` panicked with
   `failed to open icon .../icons/icon.png`. The directory held `icon.ico` and nothing else.
   Tauri reads the `.ico` on Windows and a PNG everywhere else.
3. **Two `acter-core` tests asserted on `C:\`-shaped paths.** `Path::parent` finds no
   separator in `C:\tools\pwsh\pwsh.exe` off Windows, so it is one filename and the parent is
   the empty string.
4. **`the_list_asks_the_machine_again_on_every_call` failed** because the catalogue was
   `#[cfg]`-selected and empty off Windows: a service that asks the machine on every call
   never asked it at all, so the counter it watches stayed at zero.
5. **Nine `acter-app` router tests failed** with `connectable not allowed. Plugin not found`.
   The mock invoke was built with a literal `http://tauri.localhost`, which is *Windows'*
   local origin; macOS serves `tauri://localhost`, so every invoke arrived as though from a
   remote page and Tauri's ACL refused it. Found only after 2 was fixed.
6. **Four `acter-shells` tests failed and a fifth passed vacuously**, all parsing Windows
   path shapes — `WindowsApps` package directories, `Program Files\PowerShell\7` — with
   `Path`, which off Windows sees one component and finds nothing in it.

And one thing that was not a test failure and was worse: **`profiles_directory()` fell back to
`%APPDATA%` and then to the current working directory**, so a macOS Acter would have written
its `known_hosts` and its record of explained shells into whatever directory it was launched
from. A host key verified from one directory would be unknown from the next.

## Decisions

### 1. The platform is an argument, not a `#[cfg]`

`catalogue` took a `has` predicate and read its kinds from a `#[cfg]`-selected constant, of
which exactly one existed in any build. `offered(os: &str)` replaces both constants, `catalogue`
takes the list, and `ConnectService` takes it at construction the way it already takes its
scripted profile names. The composition root reads `std::env::consts::OS`, because reading the
environment is the composition root's privilege and not a service's.

**What that buys is not tidiness, it is coverage.** Before this, the macOS rules could only be
asserted on a Mac and the Windows rules only on Windows, so whichever machine a developer
changed the connect list on, half of what they changed was unasserted. Failure 4 is what that
costs in practice. **Eleven tests in `services/connect.rs` were `#[cfg(windows)]` for want of
exactly this line** — the connect list, WSL's row and its three reasons, PowerShell's editions,
the not-verified label, telling two installs apart — and ten of them now run everywhere with no
change but the gate removed.

This is ARCHITECTURE's platform-divergence rule in its mildest form: the answer is a value, so
it needs no gate, no adapter and no port. A port for a constant list would be the trait wrapper
that document's guardrail already forbids.

### 2. macOS offers SSH, and offering only SSH is the point

`ON_MACOS` is one kind. Acter speaks SSH itself rather than running a client (spec B9,
decision 1), so `russh` is portable, `KnownHosts` reads paths, and `users_known_hosts()`
already fell back to `HOME` — **the kind that would have been hardest to port is the one that
needed no porting at all.** So the smallest honest macOS build is one that connects somewhere
real, which is what makes everything after it judgeable against a working window rather than
against a compile.

cmd, PowerShell and WSL are absent rather than unavailable, which is the rule the catalogue's
own doc comment has carried since B5.4 finally meeting a second platform.

### 3. A Windows-shaped path in a test is Windows' spelling, not the rule

Failures 3 and 6 are one mistake in seven places: a literal like
`C:\Program Files\WindowsApps\...\pwsh.exe` is a *path* on Windows and a *filename* everywhere
else. What those tests are about — that an install can say where it lives, that a package
family is read from the directory above the file, that a name with no extension still names
one — is true on every platform.

So the separators became the platform's own (`["Program Files", WINDOWS_APPS, ...].iter().collect()`)
and the expectations are built from the same pieces as the paths. **The directory *names* stay
as they are**, because those are the Windows knowledge under test.

**One of the seven was passing while asserting nothing**, which is the reason this decision is
not merely a repair: `a_file_somewhere_that_says_nothing_is_reported_as_saying_nothing` was
green on macOS because a single unparsed component is indeterminable for the wrong reason. A
vacuous green test is worse than a red one.

**Four `acter-shells` tests stay `#[cfg(windows)]` and should**: their subject is a registry
key or a real system directory, and there is nothing on a Mac for them to be about.
Likewise `real_session.rs`'s `what_this_machine_actually_has` module, whose subject is
`WindowsTrust` — one gated `mod`, because macOS answers the same port with an adapter of its
own and that adapter's tests will be its own (roadmap 33).

### 4. The mock webview is asked for its own URL

Failure 5 was a literal standing in for something the harness already knows. `mock.webview.url()`
is what the platform actually serves, on every platform, so the invoke arrives from the origin
Tauri's ACL expects. No gate, and one fewer thing that can be right on one platform only.

### 5. Records go where the operating system keeps them

`records_directory(os, appdata, home)` is pure and tested; `profiles_directory()` reads the two
variables at the edge and calls it. macOS answers `~/Library/Application Support/acter` — the
place Finder, Time Machine and every native application agree on, rather than a dotfile, whose
convention is Unix's and not this platform's. An operating system with no answer gets `None`
and the caller's working-directory fallback, which is the behaviour that already shipped.

**It is pure so that it can be wrong loudly.** Reading the environment inside a `#[cfg]` is
what made the old answer invisible until somebody ran the product on the platform it was wrong
for.

### 6. Two build-script functions, not one gated block

`build.rs` declared `attributes` as `mut` and mutated it only inside `#[cfg(windows)]`, so
every non-Windows build carried an `unused_mut` warning — and CI runs clippy with
`-D warnings`, so the macOS job would have failed on it. Two `attributes()` functions, one gate
each: ARCHITECTURE's rule in its middle form.

### 7. The macOS CI job is the Rust half only

The frontend build, its typecheck, its 316 vitest tests and the protocol-binding drift guard
are platform-independent and already run on the Windows job; running them twice buys minutes
and nothing else. What the macOS job exists to catch is the half that *is* platform-dependent,
which before this entry was untested anywhere. The E2E suite stays Windows-only until there is
a macOS window worth driving (roadmap 34).

## Files touched

- `crates/acter-core/src/policies/catalogue.rs` — `offered(os)`, the two per-platform lists,
  `catalogue` taking its kinds, and tests that assert every platform from any machine.
- `crates/acter-core/src/policies.rs`, `crates/acter-core/src/lib.rs` — `offered` re-exported.
- `crates/acter-core/src/services/connect.rs` — the `kinds` field and constructor argument;
  eleven `#[cfg(windows)]` test gates removed; one install-directory test made native.
- `crates/acter-core/src/entities/shell_install.rs` — the `under` helper and the two tests that
  needed it.
- `crates/acter-shells/src/installed.rs`, `crates/acter-shells/src/installed/roots.rs` — the
  `at` helper and the five path-shape tests, one of which was vacuous.
- `crates/acter-transports/tests/real_session.rs` — `what_this_machine_actually_has` gated to
  Windows; the three `ConnectService::new` sites name Windows' kinds.
- `crates/acter-app/src/container.rs` — `offered(consts::OS)` wired in; `state_on`;
  `records_directory`; four new tests.
- `crates/acter-app/src/routers.rs` — the mock invoke uses the webview's own URL.
- `crates/acter-app/build.rs` — two `attributes()` functions.
- `crates/acter-app/icons/icon.png` — the same artwork as `icon.ico`, in the format Tauri reads
  off Windows.
- `.github/workflows/ci.yml` — the macOS job.
- `docs/ROADMAP.md` — entry 32 flipped to Done, and its "four things" corrected to six.

## Definition of done

- `cargo check --workspace --all-targets` clean on macOS.
- `cargo test --workspace` green on macOS: **614 tests, 0 failures** (was: `acter-app` did not
  compile, and 3 of `acter-core`'s 298 failed).
- `cargo clippy --workspace --all-targets -- -D warnings` clean on macOS; `cargo fmt --check`
  clean.
- The frontend is untouched and its suite passes on macOS unchanged: `tsc --noEmit` clean, 316
  vitest tests green, `ui/src/protocol.ts` unchanged by the generator.
- The Windows job still passes, which is what says none of this was bought at Windows' expense.
- A macOS build launches, opens the unconnected window, and its Connect dialog offers SSH and
  the debug scripted sessions and nothing else.

## What was observed on a real macOS build

Built with `--features custom-protocol` against the real frontend and launched, 2026-08-31.
**The window could not be screenshotted** — this terminal has no Screen Recording permission,
so `screencapture` returns the desktop for every application including Safari — so it was read
through macOS's own accessibility API instead, which is the API VoiceOver reads and therefore
the better evidence for this product:

- The window exists as `AXStandardWindow`, titled `Acter`, 900x600.
- Its tree carries the heading `Acter`, the text `Not connected.`, a `Connect` button, and a
  `Connection status` group reading `not connected` — the A10 unconnected window, whole.
- Activating `Connect` opens the dialog. Its `Connection kind` list reads **SSH**, then
  `Scripted: builtin`, `Scripted: builtin-by-byte`, `Scripted: unmarked`,
  `Scripted: unmarked-by-byte`. **No Command Prompt, no PowerShell, no WSL** — decision 2,
  observed rather than argued.
- The `Connection details` panel carries `Host`, `Port` (with its incrementor) and `Account`
  text fields, and the set-up checkbox reads
  `Let Acter set this session up so it can tell you more about what you run`.

**The in-document menu bar is correctly absent**, since `main.ts` removes it off Windows — and
what stands in its place is Tauri's *default* macOS menu (File, Edit, View, Window, Help), with
no Connect, Help or About item in it. That is roadmap entry 34's subject, observed here so that
entry starts from a measurement.

## Accessibility checklist for the PR body

**VoiceOver on macOS 15, and none of it is agent-observable yet**: there is no screen-reader
bridge configured on this machine, so every item below is the human's to run and check. The
bridge is being set up after this PR is opened; if it is connected in time, the items that are
speech-only become agent-observable and will be recorded with the reader version and capture
mode, as CLAUDE.md requires.

- The window opens and VoiceOver announces it as Acter, not connected.
- VO-arrowing the unconnected window reaches the Connect button and its state is spoken.
- Connect opens the dialog and VoiceOver announces it as a dialog.
- Arrowing the connection-kind list reads SSH, and reads no Windows kind at all.
- Tab from the kind list lands in the panel, and Host, Port and Account are each announced with
  their label.
- The set-up checkbox is announced with its state.
- Escape closes the dialog and focus returns somewhere a listener can hear.

## What implementing it did not settle

**Acter is not an app bundle on macOS.** `bundle.active` is `false`, so what is built is a bare
executable. It runs, it windows and it is accessible — but it has no `Info.plist`, no icon in
the Dock, and cannot be signed or notarised. Roadmap entry 35.

**The default macOS menu is Tauri's, not Acter's.** Roadmap entry 34.

**Nothing local can be connected to yet**, which is the entry's own scope and roadmap entry 33.
A macOS user gets SSH and the scripted sessions, and the connect list says exactly that rather
than implying more.

**The measured machine is Intel.** `x86_64-apple-darwin`, macOS 15.0. Apple Silicon is
unmeasured — nothing here is architecture-dependent as far as anyone can see, but "as far as
anyone can see" is what this project spends measurements to avoid saying. The CI job runs
`macos-latest`, which is Apple Silicon, so the first CI run is that measurement.
