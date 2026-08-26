# B9 — SSH: a far end that is not on this machine

Roadmap entry 27. **Drafted 2026-08-25 for review**, and deliberately not implemented: the
questions at the end are the user's, and several of them decide the shape of everything
above them.

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

### 2. SSH is a transport, and bash-over-SSH is the bash adapter

DESIGN says combinations are free: the shell adapter and the transport are different axes.
So this entry builds an `SshTransport` behind the existing `Transport` port and reuses
whichever `ShellAdapter` the far end runs — the same bash injection B5.3 measured under WSL.

Two things the port already accounts for, which is evidence the seam is in the right place:
`interrupt` is a method rather than a byte because over SSH it is a channel request, and
`resize` is a method because a window change is a protocol message.

**What has to be measured rather than assumed**: whether the injection survives a remote
`.bashrc` the way it did under WSL, and whether OSC 133 markers cross the connection intact
(DESIGN says they should — they are just bytes in the output stream — and B4.5's lesson is
that "should" is not evidence).

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
inspectable and portable. **Not the user's `~/.ssh/known_hosts`**: writing to a file other
tools depend on is a side effect a terminal has no business having, and reading it is the
open question below.

### 6. Connecting is asynchronous and says so

An SSH connection takes seconds and can hang. It reports progress as it goes — resolving,
connecting, authenticating — because a listener with no feedback cannot tell a slow network
from a dead one. This is the same gap roadmap 23.7 filed for local shells that start slowly,
and the two should share one mechanism rather than inventing a second.

## Files touched (sketch)

- `crates/acter-transports/src/ssh.rs` — the transport (new).
- `crates/acter-core/src/ports/driven/` — a port for the questions SSH asks: host key
  verification and credential prompting, so the domain never opens a dialog itself.
- `crates/acter-app/src/routers/` — the commands the dialogs invoke.
- `ui/` — the host-key dialog and the credential dialog.
- The Connect dialog (A8) gains SSH as a kind with a form.

## Definition of done (sketch)

- [ ] A session over SSH to a real server, with markers measured across the connection.
- [ ] An unknown host key is announced, decided by the user, and remembered.
- [ ] A **changed** host key is announced differently, and refusing it is the default.
- [ ] A password never reaches the buffer, the announcer, a log, or the debug tape.
- [ ] Interrupt and resize cross the connection.
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
remote still has to assign it, which means B9 cannot open a plain shell channel and set an
environment variable — it has to *request a command*, and then the question becomes whether
what that command sets survives the remote `.bashrc` that runs after it. That is the next
measurement, and the rig is built to take it.

## Questions for you, before any of this is built

1. **Do we read the user's existing `~/.ssh/config` and `known_hosts`?** Reading config
   would let `acter ssh myhost` mean what it means everywhere else, which is a real
   convenience — at the cost of parsing a format with a great many directives, most of
   which we would silently ignore. My inclination is to **read `known_hosts` and not write
   it**, and to leave `config` for later. What do you want?
2. **Agent support in the first version, or keys and passwords only?** Pageant and
   OpenSSH's agent both exist on Windows and they are different protocols. Skipping the
   agent makes the first version much smaller; including it is what most people actually
   use day to day.
3. **Where do passphrases live between connections?** Nowhere (ask every time), in memory
   for the life of the process, or in the Windows Credential Manager? The middle option is
   the usual compromise and the one I would default to.
4. **Is a jump host / ProxyJump in scope at all?** It is common in the environments where
   people live in terminals, and it multiplies the auth conversation by the number of hops.
5. **How much does this entry take on?** It could be "one host, password auth, a session"
   and grow, or the whole auth surface at once. I would rather ship the first and let the
   accessibility checklist for each auth method arrive with the method itself.
6. **How does the shell integration reach the far end?** Added 2026-08-26, after the
   measurement above: there is no environment carrier, so the choice is between requesting a
   command instead of a shell (`bash` started with the setup already applied), writing the
   setup into the channel after connecting and swallowing its echo, or accepting that an SSH
   session is unintegrated and degrades the way DESIGN's reliability case 2 describes. The
   third is a real option and should not be dismissed: it costs command boundaries, which is
   most of what non-interactive mode is.
