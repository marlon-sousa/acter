# Five fixes before the 0.1 beta

Roadmap entries 47, 50, 44, 42 and 28.12, in one spec and one PR, as the user asked on
2026-09-26 ahead of entry 55, the 0.1 beta. Entry 50 is in lane 1; the others are in lane 2,
and 28.12 is under keyboard routing. They are ordered by how much a beta tester would meet
each one.

Four of them were found by reading code during lane 4's comment work (C4, C6, C7, C10), and
none of those four had been observed. 28.12 was raised by the reader pass on 28.11. Each is
reproduced below, on 2026-09-26, before anything was changed.

## 47 — a password is asked for before the server says it takes one

### What the entry recorded

Found on 2026-09-23 while stripping comments for C7 (PR #69), by reading the code. The C7
spec's list of stale comments had already named it.

`authenticate` in `crates/acter-transports/src/ssh/transport.rs` tells the listener
"Signing in." and then, on the first pass of its loop, asks `SshQuestions::password` without
asking the server which methods it offers. `offers_password` is consulted only after
`authenticate_password` has been rejected, to decide whether to ask again. The doc comment
removed in C7 said the password is "asked only when the server will take one", which is not
what the code does.

The entry predicted this: on a server that does not offer password authentication (keys only,
for example), the person is asked for a password, types one, and only then hears that the
server "would not accept that password … and will not accept another one".

### Measured, 2026-09-26

Against the Debian `sshd` rig in `docker/ssh`, run a second time on port 2223 with
`PasswordAuthentication no`:

- OpenSSH's own client answered `Permission denied (publickey)`, so the server offered keys
  only.
- Acter said "Connecting to 127.0.0.1." and "Signing in.", asked for the password once, and
  ended with "The server at 127.0.0.1 would not accept that password for acter, and will not
  accept another one. Check the account name and the password, then try again." That last
  piece of advice is wrong: no password would ever have worked.

### Decisions

1. **Before any password, Acter asks the server what it takes.** This is the SSH `none`
   request that every OpenSSH client sends first; OpenSSH does not count it against its
   limit on attempts. Its answer is one of three:
   - the server lets the account in with nothing, and Acter is signed in;
   - the server takes a password, and Acter asks for it exactly as before, retries included;
   - the server does not take one, and nobody is asked for anything.
2. **What the listener hears when the server takes no password**, agreed in conversation on
   2026-09-26: "The server at {host} does not take a password for {user}, and Acter can only
   sign in with a password so far." Signing in with a key is B9.1 and stays out of scope.
3. **The rig can be a server that takes no password.** `docker/ssh/entrypoint.sh` reads
   `ACTER_SSH_KEYS_ONLY=1`, the way it already reads `ACTER_SSH_REKEY=1`, and the README
   gives the `docker run` line for port 2223.

## 50 — the far-end toggle depends on the keyboard layout

### What the entry recorded

Found on 2026-09-23 while stripping comments for C10 (PR #70), by reading the code; not tried
on a non-QWERTY layout.

`isFarEndToggle` in `ui/src/adapters/keyboard.ts` recognised Ctrl+Shift+K by
`event.key === 'k' || event.key === 'K'` with `ctrlKey` and `shiftKey`. The comment above it,
deleted in C10, said the toggle is matched on the physical letter rather than on `event.key`.
The code did the opposite: `event.key` is the character the active layout produces, and
`event.code` (`KeyK`) is the physical key. On a layout where the key labelled K produces
another character, or where K sits elsewhere, the toggle followed the character, not the key
position the comment described.

### Measured, 2026-09-26

The same file has the same pattern in two more places:

- `isReportable` matched Ctrl+C and Ctrl+D from the edit field on `event.key`, so on a
  Russian layout the interrupt was never reported.
- `keyOf`, for the far-end field, reported Ctrl plus whatever character the key typed.
  `policies::key_bytes` turns Ctrl plus a Latin letter into a control byte and sends any other
  character as UTF-8. So on a Russian layout, Ctrl+C in far-end mode would have typed "с" into
  the shell instead of stopping the command.

F1, F6, Escape and the named keys do not depend on the layout. Two jsdom tests failed against
the old code: a keydown with `key` "Л" and `code` `KeyK` did not toggle, and one with `key`
"с" and `code` `KeyC` was not reported. Those tests assume what WebView2 reports; Chromium
reports the layout's character in `key` while Ctrl is held. That was not observed on a
Russian layout on this machine, because doing so means adding the layout to Windows.

### Decisions

1. **Accept either, the way Windows does.** A chord holding Ctrl without Alt is matched on the
   letter the key types when that letter is Latin, and otherwise on the letter at the key's
   position (`event.code`). A Latin letter wins because Windows' own shortcuts follow it: on
   Dvorak, Ctrl+C is the key printed C, not the key at QWERTY's C position. Matching on
   position alone would break that.
2. **Not with Alt.** On Windows AltGr arrives as Ctrl+Alt, and the character it typed
   (Polish "ą") is what was meant.
3. **All three places are in scope**: the toggle, Ctrl+C and Ctrl+D from the edit field, and
   Ctrl plus a letter in the far-end field. Nothing the listener hears changes. DESIGN's
   keystroke map gains the rule.

## 44 — a signature whose subject merely contains "Microsoft" is announced as Microsoft's

### What the entry recorded

Found on 2026-09-23 while stripping comments for C6 (PR #68), by reading the code; no such
certificate has been tried on a real machine.

In `crates/acter-shells/src/windows_signatures/trust.rs`, `verdict_for` maps a status of 0
from `WinVerifyTrust` to `Verdict::Trusted`, and picks the signer from the certificate
subject's simple display name (`CertGetNameStringW`):
`Some(name) if name.contains(MICROSOFT) => Signer::Microsoft`, where `MICROSOFT` was
`"Microsoft"`. The doc comment on `MICROSOFT` said a prefix was matched; the code matched
anywhere in the name.

What follows from `Signer::Microsoft`, in `crates/acter-core/src/entities/signature_verdict.rs`:

- `Verdict::said` gives "This computer trusts this file's signature, and Microsoft signed it.
  There is nothing to decide before starting it."
- `Verdict::note` gives `None`, so nothing is said about the signer at connection.

`Signer::Other { name }` instead gives "… it was signed by {name}. Start it if that is who you
expect to have built it." and the note "signed by {name}". So a certificate that chains to a
root this machine trusts, issued to any subject whose name contains "Microsoft", was presented
to the listener as Microsoft's. Whether the file is trusted is unaffected: `settled` is true for
every trusted verdict, whoever signed it.

### Measured, 2026-09-26, Windows 11 Pro 26200

| File | Signed as | How | Chain ends in |
| --- | --- | --- | --- |
| `System32\cmd.exe` | Microsoft Windows | catalog | Microsoft Root Certificate Authority 2010 |
| `System32\WindowsPowerShell\v1.0\powershell.exe` | Microsoft Windows | catalog | Microsoft Root Certificate Authority 2010 |
| `System32\wsl.exe` | Microsoft Windows | catalog | Microsoft Root Certificate Authority 2010 |
| `pwsh.exe`, Store, 7.6.6 | Microsoft Corporation | embedded | Microsoft Root Certificate Authority 2011 |
| `Program Files\WSL\wsl.exe` | Microsoft Corporation | embedded | Microsoft Root Certificate Authority 2011 |

Windows' own Microsoft-root check, `CertVerifyCertificateChainPolicy` with
`CERT_CHAIN_POLICY_MICROSOFT_ROOT`, accepted the three 2010 files with no flags and rejected
them with `MICROSOFT_ROOT_CERT_CHAIN_POLICY_CHECK_APPLICATION_ROOT_FLAG`. It did the reverse
for the two 2011 files. Git for Windows' `bash.exe`, signed by Johannes Schindelin, failed
both. A unit test giving the name "Not Microsoft Ltd" came back as `Signer::Microsoft`.

### Decisions

1. **Microsoft's is an exact name and a Microsoft root, both required.** The simple display
   name must be exactly "Microsoft Windows" or "Microsoft Corporation", and the signer's chain
   must pass the Microsoft-root check in either of its two forms.
   - The name alone is not enough, because any root this computer trusts, a company's own
     included, can issue a certificate named "Microsoft Corporation".
   - The root alone is not enough, because Microsoft's roots are believed to sign other
     publishers' Store apps under the publisher's own name. That was not measured, and the
     rule does not depend on it.
2. **No new sentence.** Anything else that is trusted is `Signer::Other`, and is announced with
   the existing "signed by {name}" wording.

## 42 — a file started anyway goes unmentioned when the far end also has a note

### What the entry recorded

Found on 2026-09-23 while stripping comments for C4 (PR #67), by reading the code; nothing
observed with a screen reader.

In `crates/acter-core/src/services/connect.rs`, `use_profile` builds two optional clauses for
the connection announcement:

- `agreed`, from `verified`: the "started although …" clause that `Verdict::note` gives for a
  file that did not verify and that the user chose to start anyway;
- `note`, from the factory's `Started`: what the far end is, from `far_end_note` in
  `crates/acter-app/src/container.rs`.

It kept one: `let note = note.or(agreed);`. The comment above that line, deleted in C4, said
at most one of the two is ever present. That is false. `far_end_note` gives a note for SSH, for
WSL (`container.rs`, `fn wsl`) and for a Unix login shell (`fn unix_shell`) whenever the shell
was detected. WSL starts `wsl.exe`, the macOS Terminal kind starts a login shell, and both are
files `verified` checks. When such a file did not verify and the user chose Start anyway, the
announcement said what the far end is and nothing about the file having been started
unverified.

### Measured, 2026-09-26

A unit test with a factory that gives the note "bash", and a file answering "not signed" that
the user starts anyway, got the note "bash" alone: "started although nothing has signed it"
was dropped. A trusted non-Microsoft signer's "signed by {name}" is dropped the same way.

### Decisions

1. **Both are said. The far end's note comes first, and the signature clause follows it as a
   sentence of its own**, agreed in conversation on 2026-09-26:
   - "connected to Ubuntu, bash. Started although nothing has signed it."
   - "connected to Ubuntu, bash. You will hear what commands print here, but not whether they
     worked. Started although nothing has signed it."
2. Either clause alone is said exactly as before.

## 28.12 — Ctrl+C at an idle prompt says a command failed

### What the entry recorded

Raised on 2026-09-02 by the reader pass on 28.11, at an integrated WSL Ubuntu.

Nothing is running. The listener presses Ctrl+C, to clear a line they have half-typed or to
feel where they are, and hears "command failed, exit code 130". Nothing failed, because nothing
ran.

28.11 does not cover it, and this was measured rather than assumed. bash echoes `^C` onto the
command line before drawing the next prompt, so the block that opens is a named one: it has a
command line, and 28.11's rule deliberately leaves every named block its verdict. It is also
said once rather than at every Enter, since the Enters after it are quiet, so this is not the
loop 28.11 closed.

What it is really about is A3.2. `KeyAck::NothingToActOn` already exists for exactly this key
at exactly this moment, and the frontend already has a sentence for it. So the question is not
how to silence a verdict. It is whether the ack and the shell's 130 should be allowed to
describe the same keypress twice, and which of the two a listener is better served by. The
answer belongs in A3.2's terms, without a third rule beside it. Whether it is a defect at all
was the user's to say: 130 is the truth about `$?`, and a listener who pressed Ctrl+C did cause
it.

### Measured, 2026-09-26

Real cmd, Windows PowerShell and bash under WSL (Ubuntu 24.04), through the whole stack. Each
first ran `echo acter-ready`; once it had finished, Ctrl+C was pressed:

| Shell | Acter holds the line | The far end holds the line |
| --- | --- | --- |
| bash | ack `NothingToActOn`: "nothing running to stop" | a block headed `^C`, then "command failed, exit code 130", then the prompt |
| cmd | ack `NothingToActOn`: "nothing running to stop" | its prompt, read aloud |
| PowerShell | ack `NothingToActOn`: "nothing running to stop" | its prompt, read aloud |

In far-end mode, bash over a half-typed "echo half" did the same, with the heading
`echo half^C`. Local mode already behaves as A3.2 intends, so the defect is far-end mode only.
Far-end mode is the default for a new connection.

### Decisions

1. **It is a defect, and it is answered in A3.2's terms**, agreed in conversation on
   2026-09-26. A Ctrl+C with nothing running is described at most once, and never as a
   command's outcome. When Acter holds the line, the ack describes it: "nothing running to
   stop", unchanged. When the far end holds the line, the key still goes to the far end,
   because clearing the line is its line editor's job, and Acter says nothing of its own. The
   listener hears the prompt come back, the same as with cmd and PowerShell.
2. **How the service knows.** When the key it sends to the far end is `0x03` and nothing is
   running, it remembers that. The next block to close then carries no exit code, so no
   failure is announced for it. Any other key sent to the far end forgets it, because
   PowerShell opens no block at all and a later command's failure must still be said.
3. DESIGN's layer 2 gains the rule.

Options not taken:

- Leaving it, so that bash alone calls the keypress a failure.
- Answering "nothing running to stop" in far-end mode too. The ack would claim nothing
  happened while the key did reach the far end and cleared its line.

## Files touched

- `crates/acter-transports/src/ssh/transport.rs`: `authenticate` asks the server first (47).
- `crates/acter-transports/tests/ssh_rig.rs`: the key-only rig test (47).
- `docker/ssh/entrypoint.sh`, `docker/ssh/README.md`: `ACTER_SSH_KEYS_ONLY` (47).
- `ui/src/adapters/keyboard.ts`, `ui/test/adapters/keyboard.test.ts`: `chordLetter` (50).
- `crates/acter-shells/src/windows_signatures/trust.rs`: the exact names and the
  Microsoft-root check (44).
- `crates/acter-shells/src/windows_signatures.rs`: real-file tests for both `wsl.exe` and for
  Git's `bash.exe` (44).
- `crates/acter-core/src/services/connect.rs`: `both` (42).
- `crates/acter-core/src/services/session.rs`: `cleared_at_idle` (28.12).
- `crates/acter-core/src/controllers/session_actor.rs`: `SessionInput::CommandEnded` carries
  `Option<ExitCode>` (28.12).
- `crates/acter-transports/tests/real_session.rs`: Ctrl+C at an idle prompt, against real
  shells (28.12).
- `docs/DESIGN.md`: the keystroke map's two rules (50, 28.12).
- `docs/ROADMAP.md`: the five lines point here; the five entry files are deleted.

## Acceptance criteria

- On a server that takes no password, nobody is asked for one, and the attempt ends with the
  sentence in 47's decision 2. A server that takes a password behaves exactly as before,
  including the retry.
- Ctrl+Shift+K, Ctrl+C and Ctrl+D work on a layout whose letters are not Latin. The far-end
  field sends a control byte for Ctrl plus such a key, and still sends what AltGr typed.
- A trusted signer is Microsoft's only under one of the two exact names and a Microsoft root.
  cmd.exe, powershell.exe, pwsh.exe and both wsl.exe files still are; Git's bash.exe is not.
- A file started anyway is always mentioned at connection, whatever the far end says.
- In far-end mode, Ctrl+C at an idle bash prompt is not announced as a failure, and a command
  that fails afterwards still is. In local mode, cmd, PowerShell and bash still answer
  "nothing running to stop".
- Each reproducing test fails against the old code: `a_name_that_merely_contains_microsoft_is_somebody_else`,
  `a_file_started_anyway_is_mentioned_beside_what_the_far_end_is`, the layout tests in
  `keyboard.test.ts`, `a_ctrl_c_with_nothing_running_is_not_said_to_have_failed`, and the rig
  test `a_server_that_takes_no_password_is_never_asked_for_one`.
- `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test --workspace`, the UI tests and the
  ignored real-shell tests are green.

## Manual accessibility checklist (PR body)

Through the screen-readers bridge, `user` persona, one item per sentence that changes:

- SSH to the key-only rig on port 2223: the listener hears "Signing in." and then the sentence
  in 47's decision 2, and no password dialog opens.
- SSH to the password rig on port 2222: the password dialog still opens, and a wrong password
  still asks again.
- A far-end note and a started-anyway clause together are heard as one announcement, in the
  order of 42's decision 1.
- WSL bash, far end holding the line: Ctrl+C at an idle prompt is followed by the prompt and
  no failure.
- cmd and PowerShell, far end holding the line: Ctrl+C at an idle prompt is followed by the
  prompt, as before.
- Acter holding the line: Ctrl+C at an idle prompt says "nothing running to stop".

## Out of scope

- Signing in with a key or through an agent (B9.1, B9.2).
- The rig test `what_a_session_says_when_it_has_just_connected`, which fails on main as well.
  It is entry 27.7.
- A Ctrl+C sent to the far end while a command is running. What that says is unchanged.
