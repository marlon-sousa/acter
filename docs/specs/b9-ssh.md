# B9 — SSH: a far end that is not on this machine

Roadmap entry 27. Drafted 2026-08-25, **agreed 2026-08-26** after the five open questions
were decided and the rig in `docker/ssh/` had measured the one that mattered most. One of
those answers reverses this spec's own first recommendation, and the reversal is written
where the recommendation was rather than quietly replacing it.

## Why this is different from every transport before it

`LocalPty` spawns a process and the operating system does the rest. SSH has to *establish*
something first, and establishing it can fail in ways that are not errors: a host key nobody
has seen before, a password nobody has typed yet, a key file with a passphrase, an agent
that may or may not be running. Each of those is a **question the far end asks the user**,
before there is a session to ask it in.

That is the whole difficulty, and it is an accessibility difficulty rather than a networking
one. Every earlier transport could report failure as one speakable sentence. This one has to
conduct a conversation.

## Decisions

### 1. A library, not `ssh.exe`

**`russh`**, which DESIGN already names. The reason is not portability, though SSH is the
one connection kind that is the same on every platform and `ssh.exe` is a different program
with different output on each.

The reason is that **prompts become return values**. `ssh.exe` writes "Are you sure you want
to continue connecting (yes/no/[fingerprint])?" and a password prompt to the terminal it
owns; Acter would have to recognise localised, version-dependent English in a byte stream to
know a question had been asked, and a screen reader user's answer would depend on our having
matched that text. With a library, an unknown host key is a callback and a password is a
typed method call — so Acter can put a real, labelled dialog in front of the user and know
what it is for.

It also makes failure a typed error rather than an exit code plus stderr text, which is what
the Connect dialog needs in order to say what went wrong (spec A8, decision 4).

**What this costs, stated plainly**: Acter owns key parsing, host-key storage, agent
protocol and reconnection. `ssh2` (libssh2 bindings) would move some of that into C at the
price of a build dependency and a blocking API that fits `Transport` less well. `russh` is
pure Rust, async, and already in DESIGN.

### 2. SSH is a transport, and the session it carries is unintegrated

DESIGN says combinations are free: the shell adapter and the transport are different axes.
So this entry builds an `SshTransport` behind the existing `Transport` port and reuses
whichever `ShellAdapter` the far end runs — the same bash injection B5.3 measured under WSL.

Two things the port already accounts for, which is evidence the seam is in the right place:
`interrupt` is a method rather than a byte because over SSH it is a channel request, and
`resize` is a method because a window change is a protocol message.

**How the injection reaches the far end: it does not — Decided 2026-08-26.** An SSH
session is **unintegrated**. Acter opens a plain shell channel and makes no attempt to make
the far end emit markers.

This was decided after measuring, and against the earlier draft of this section, which chose
to request a command instead. The measurement is below under "the injection has no carrier":
there is no environment variable a client can send that an unmodified server will pass
through, because OpenSSH's stock `AcceptEnv` is `LANG` and `LC_*` and a server belonging to
somebody else has not been configured for us.

The two ways around that were each refused for what they cost:

- **Requesting a command rather than a shell** — starting bash with the setup already
  applied. It works against an unmodified server, but it replaces the user's login shell
  invocation with ours, and whether what it sets survives the remote `.bashrc` that runs
  afterwards was still unmeasured. It buys integration by making every SSH session subtly
  not the session `ssh` would have given.
- **Writing the setup into the channel after connecting** needs no server cooperation at
  all, but the first thing the session would do is type a line the user did not type, and
  swallowing that echo is B4.9's whole subject — a hard problem re-entered for no gain.

**Prior art says this is the ordinary answer, not a shortfall.** VS Code documents that its
automatic injection does not work through a regular `ssh` session and tells the user to
install its script on the remote. WezTerm ships an SSH client and leaves remote integration
to the user. iTerm2, which invented shell integration, says "you should do this on every
host you ssh to". Only kitty automates it, and it does so by transmitting a compressed
tarball over the TTY and unpacking it with a bootstrap script — the scale of machinery that
answer actually costs. Three of four mature terminals ship exactly what this decision ships.

**What an unintegrated session still is**, which is most of it: output rendered into the
buffer and readable, keys and interrupt and resize crossing the connection, the edit field,
the transcript. What it loses is command boundaries — so no blocks, no exit codes, and no
autoread of a finished command.

**Acter has to say which state it is in**, and this is the accessibility requirement rather
than a nicety. A listener cannot distinguish "this session has no boundaries" from "the
markers broke" from "Acter is broken": all three sound like silence where an announcement
should have been. B6 already announces shell integration as unavailable, and an SSH session
uses that same sentence — said once, at connection, not repeated per command.

**A later entry offers to fix it at the far end.** A button that writes the integration
snippet into the remote account's shell startup, with the user's consent, so the *next*
connection to that host is integrated — the same bargain iTerm2 and VS Code offer, made
reachable instead of documented. Deferred rather than refused, and it is why the marker
question below stays open rather than being struck out.

### 3. Host key verification is a dialog, and the default is refusal

An unknown or changed host key is the security decision in SSH, and it is exactly the moment
where a terminal traditionally prints a wall of text nobody reads. For this audience it has
to be a **modal dialog with a real accessible name**, stating in order: which host, that its
key is unknown (or *changed*, which is a different and more serious sentence), the
fingerprint in a form that can be read character by character, and what the choices mean.

**A changed key is not the same dialog as an unknown one.** An unknown key is routine; a
changed key means either the server was rebuilt or somebody is between you and it, and the
dialog says so rather than offering a cheerful "continue?".

**Acter never silently trusts.** There is no "accept everything" default, and a session
whose key was refused reports that as its speakable failure.

### 4. Passwords and passphrases never pass through the command line

A password typed into the ordinary edit field would be rendered into the buffer and read
aloud by the screen reader — the failure DESIGN already names as an open question. So
authentication input is its own dialog with a masked field, never the session's edit field,
and the value goes straight to `russh` without touching the terminal buffer, the transcript
recorder, or any log.

**The debug event recorder must not see it either.** That recorder exists to reconstruct
event ordering (spec A3.2), and a password in a debug tape is a password on disk.

### 5. What is stored, and where

Nothing, until B8's profile store exists — and even then, **never a password**. What a saved
SSH connection may hold: host, port, user, and a path to a key file. A passphrase or password
is asked for each time unless the platform's own credential store is used, which is its own
entry and not this one.

Host keys are Acter's own file, in the profile directory, in `known_hosts` format so it is
inspectable and portable. **Acter reads the user's `~/.ssh/known_hosts` and never writes it**
— Decided 2026-08-26. Reading it means a host they already trust connects without being asked
again, which is what they expect and what stops a populated `known_hosts` turning into a
sequence of prompts. Not writing it means a bug here breaks Acter and not their `ssh`.

`~/.ssh/config` is deliberately not read in this entry. Making `acter ssh myhost` mean what it
means everywhere else is a genuine convenience, and it costs parsing a format with a great
many directives most of which would be silently ignored — and silently ignoring a directive a
user has relied on is a worse failure than not reading the file at all. Its own entry.

**A passphrase lives in memory for the life of the process** — Decided 2026-08-26. Asked once
per launch, held, never written anywhere, gone when Acter closes. Asking every time is safest
and genuinely hostile now that B7 has made reconnecting an ordinary action; the Windows
Credential Manager survives restarts and is the platform's own store, but persisting a secret
deserves its own consent flow and its own accessibility pass, so it is a later entry rather
than a default.

### 6. Connecting is asynchronous and says so

An SSH connection takes seconds and can hang. It reports progress as it goes — resolving,
connecting, authenticating — because a listener with no feedback cannot tell a slow network
from a dead one. This is the same gap roadmap 23.7 filed for local shells that start slowly,
and the two should share one mechanism rather than inventing a second.

### 7. What the far end is, asked once, on a channel of its own — Decided 2026-08-26

Before the session channel is opened at all, Acter asks the far end what it is on a channel
of its own: `$SHELL`, then `$0`, then the version variables `$BASH_VERSION`,
`$ZSH_VERSION`, `$FISH_VERSION`. The first is the account's configured shell, the second is
what is actually running and carries a login shell's leading `-`, and the third is what a
shell says about itself and is the most certain.

**Between authentication and the session channel**, which is a window that exists because
authentication in SSH finishes at the *protocol* level, before any channel is opened. Only
`pty-req` plus a `shell` request makes sshd fork and exec the login shell. So there is a
moment when Acter is authenticated and no shell exists yet, and that is where the probe
goes: open a channel, `exec`, read, close, and only then open the session.

**That placement is what the code needs, not merely what is tidy.**
[`SessionService::start`](../../crates/acter-core/src/services/session.rs) takes
[`ShellFacts`](../../crates/acter-core/src/ports/driven/shell_adapter.rs) by value and reads
its markers at construction — the port says so deliberately: the service reads both facts
once and holds them for the life of the session. A probe answering *after* the session
exists would leave two options, constructing with facts known to be wrong or making the
facts mutable, and the second undoes the reason they travel together at all. Asking first
makes them right from the first byte.

**A second channel, not a line typed into the session.** SSH allows many channels over one
connection, and an `exec` request on a fresh one produces output that never reaches the
terminal buffer. Typing the probe into the session instead would put a command nobody typed
in front of a screen reader — B4.9's subject, and the same objection that settled decision 2
two paragraphs above. `exec` also answers the right question rather than an adjacent one:
sshd runs it through the account's own shell, so `$SHELL` there is the program a `shell`
request would have started.

**It does not make the session integrated, and decision 2 stands.** Knowing the far end is
bash does not make bash emit OSC 133; without a snippet installed there, nothing does. The
probe changes what Acter can *say* and what it *knows*, not what the far end emits.

What it buys, and each is worth having on its own:

- **A sentence with a subject.** "Shell integration is unavailable" tells a listener nothing
  they can act on. "Connected to acter-ssh. bash, with no shell integration set up on this
  host" names what they are talking to and points at the fix.
- **A correct end-of-input.** This is the one `ShellAdapter` fact that survives without
  integration and it is not cosmetic: bash ends on `0x04`, PowerShell ends on the literal
  line `exit` and on neither control byte (measured, B5.2). A session that cannot be ended
  properly is a real defect, and without the probe every SSH session would have to answer
  `None` — "Acter does not know how" — for a shell it could simply have asked about.
- **The ground 27.1 stands on.** The same probe notices a snippet *already* installed,
  whether Acter put it there or the user's own iTerm2 or VS Code setup did.

**Match the name, and fall back honestly when unsure** — which is behaviour the code
already has, once it is given a name: `adapter_for` returns
[`Plain`](../../crates/acter-shells/src/plain.rs) for anything it does not recognise, and
`Plain` is exactly "start it as it stands, inject nothing, claim nothing". Three states,
and they are three different sentences: a **measured** shell is integrated; a shell **known
of but unmeasured** (zsh, fish) is unintegrated and *named*; an **unrecognised** shell, or a
probe that failed, is unintegrated with no name. The identity may be guessed from the name;
the injection may never be — knowing a far end is zsh licenses saying so and nothing else
until a zsh injection has been measured the way B5.3 measured bash's. Roadmap 23.8 applies
the same rule to WSL, which had the same unexamined assumption.

**Advisory, and never a gate.** `exec` can be refused: a server with `ForceCommand`, a
restricted shell, or `internal-sftp` alone will not run it, and under `ForceCommand` it can
return an answer about something else entirely. An answer that is not a path is itself a
useful signal — Windows OpenSSH defaulting to cmd echoes `$SHELL` back literally — and it
falls into the unrecognised state above rather than being parsed hopefully.

Because the probe now runs *before* the session, "not a gate" has to be spelled out as a
deadline rather than assumed: it gets a short, fixed one, and a probe that has not answered
by then is abandoned, the session channel is opened immediately with `Plain` facts, and the
answer is ignored if it arrives afterwards. **The user's shell is never waiting on our
curiosity.** 23.7's rule applies with full force here, since this is the one place the
probe could add time to the seconds before a prompt, and those seconds are already the
worst in the product.

**One announcement, not two**, which is the accessibility payoff of the placement and the
reason it is worth a round trip. Probing after the session starts would mean the connection
is announced first and the shell's name arrives afterwards, interrupting it — an
asynchronous fact speaking over the thing it describes, which is 23.7's problem re-created
where it did not have to exist. Asking first means the answer is in hand before there is
anything to announce, so a listener hears one whole sentence: "connected to acter-ssh, bash,
with no shell integration set up on this host." A probe that hit its deadline simply drops
the middle clause; it never adds a second utterance.

## Files touched (sketch)

- `crates/acter-transports/src/ssh.rs` — the transport (new).
- `crates/acter-core/src/ports/driven/` — a port for the questions SSH asks: host key
  verification and credential prompting, so the domain never opens a dialog itself.
- `crates/acter-app/src/routers/` — the commands the dialogs invoke.
- `ui/` — the host-key dialog and the credential dialog.
- The Connect dialog (A8) gains SSH as a kind with a form.

## Definition of done (sketch)

- [ ] A session over SSH to a real server: output rendered, readable, correct — with no
      shell integration and nothing pretending otherwise.
- [ ] The session announces that shell integration is unavailable, once, at connection.
- [ ] An unknown host key is announced, decided by the user, and remembered.
- [ ] A **changed** host key is announced differently, and refusing it is the default.
- [ ] A password never reaches the buffer, the announcer, a log, or the debug tape.
- [ ] Interrupt and resize cross the connection.
- [ ] The far end is asked what it is on its own channel, and the answer never
      reaches the terminal buffer.
- [ ] A server that refuses `exec` still gives a working session, announced without
      the shell's name rather than not announced.
- [ ] A connection that fails says why, in one sentence a listener can act on.
- [ ] Accessibility checklist: every auth flow driven end to end with a screen reader.

## The rig, and what it has already measured

`docker/ssh/` builds a Debian container running `sshd`, with bash and Debian's own 113-line
default `.bashrc` at the far end. Its reasoning is in the files; the two things worth having
in the spec are these.

**A container rather than `sshd` on the developer's machine.** Enabling the Windows OpenSSH
server opens a listening service on a laptop that joins other networks, which is a security
decision rather than a convenience. The container binds to loopback only and — the property
that matters here — is genuinely not in Acter's process tree, which 22.6 established changes
how a far end behaves.

**The three host-key states are reachable without editing anything**, which is what makes
decision 3 testable: unknown (fresh `known_hosts`), known (connect twice), and **changed**
(`-e ACTER_SSH_REKEY=1`, which throws the container's identity away and generates a new one).

### Measured 2026-08-26: the injection has no carrier over SSH

Decision 2 says the bash injection has to be measured across the connection rather than
assumed. It was, against this rig, and the result changes what B9 has to build:

- **`PROMPT_COMMAND` does not cross.** Sent with `SendEnv`, it arrives empty. A client may
  only send variables the *server* has agreed to accept, and OpenSSH's stock `AcceptEnv` is
  `LANG` and `LC_*`. A server belonging to somebody else will not have been configured for
  us, so this is not a gap in the rig — it is the world.
- **`LC_*` does cross, intact.** `LC_ACTER='printf X'` arrived as `printf X`. That is the
  only carrier an unmodified server accepts, and it is the SSH analogue of B5.3's `WSLENV`.

**The rig's `sshd_config` is deliberately left restrictive**, with no `AcceptEnv` line at
all. A rig more accommodating than a real server would let B9 ship an injection that works
against our container and against nothing a user connects to — which is B4.5's lesson
exactly.

**What follows, and it is a real design consequence.** `LC_*` gets a value across but nothing
on the far end acts on it: bash will not treat `LC_ACTER` as `PROMPT_COMMAND`. Something
*remote* still has to assign it — which is the whole reason decision 2 leaves an SSH session
unintegrated rather than reaching for a carrier that does not exist. The variable arriving is
not the same as the far end doing anything with it, and that gap is not closable from this
side of the connection.

It stays worth knowing, because the deferred install-snippet entry closes it from the other
side: a snippet on the far end could read `LC_ACTER` and act on it, which makes this
measurement the foundation of that entry rather than a dead end.

## Scope — Decided 2026-08-26

**One host, password authentication, a session.** Plus host-key verification, which is not
optional at any size: connecting without it is connecting to whatever answers.

The reason is the accessibility checklist rather than the code. Every authentication method
brings its own flow to drive with a screen reader, and one entry carrying five of them is one
checklist nobody can complete in a single pass — which is how a checked box stops meaning
anything.

What follows, each with its own checklist when it lands:

- **B9.1 — public key authentication**, including a passphrase-protected key.
- **B9.2 — the agent.** Pageant and OpenSSH's agent both exist on Windows and are different
  protocols; it is what most people use day to day, which is exactly why it deserves its own
  entry rather than a corner of this one.
- **B9.3 — `~/.ssh/config`**, if it earns its place.
- **B9.4 — `ProxyJump`.** Common in the environments where people live in terminals, and it
  multiplies the authentication conversation by the number of hops — so it is the last of
  these rather than the first.

## Still open

- **Do OSC 133 markers cross the connection intact?** Expected, unmeasured, and not needed
  by this entry — an unintegrated session emits none. It is the first thing the deferred
  install-snippet entry has to measure, and the rig is built to take it.
- **What the install widget writes, and where.** Which file on the far end, what it does
  about a shell that is not bash, and how consent to modify somebody's remote account is
  asked for in a way a listener can refuse. Roadmap 27.1 carries the reasoning; two things
  in it are measurable on this rig today, and should be measured before that entry is
  specced. A `shell` request starts a **login** shell, so bash reads `~/.bash_profile` or
  `~/.profile` and never `~/.bashrc` — a snippet in the wrong file passes every local test
  and never runs over SSH. And `exec` on a second channel returns `$SHELL` without a byte
  reaching the session buffer, which is how the far end can be asked what it is without
  re-entering B4.9.
