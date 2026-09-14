# 36 — M5, the far end's line is audible on macOS.

Lane: lane-3-macos. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**M5, the far end's line is audible on macOS.** Spec: none yet → specify first. Raised by
the user 2026-09-03: in remote-keys mode they could hear neither the keys they typed nor
the keys they deleted. It reproduces everywhere and it is the frontend's, not the
terminal's.

**Measured 2026-09-03** (VoiceOver, macOS 15.0, silent capture, `user` persona), against
a real `bash` over SSH to the `docker/ssh` rig with the session set up, and again against
a local `bash` on this Mac with the setup skipped. The two are identical, so neither the
transport nor shell integration is in it. Typing `e`, `c`, `h`: **nothing spoken**, with
`VO-F4` reading the row back as `ech`, so every key reached the far end and its echo
arrived. `Backspace`: **nothing spoken**. What *is* spoken: the completing `Tab`
(`o`, then `não selecionadas`), every caret move (`Left Arrow` said `o`, the character it
moved over), the output of a command, and its verdict.

**The rule behind the inconsistency**: VoiceOver speaks when the caret or the selection
moves *and the text did not change*. Every render that changes the text is silent, and
takes the caret announcement down with it. NVDA never needed the text change — it speaks
typed characters from its own keyboard hook — which is exactly what spec 28, decision 2
measured and built the mode on, and it is a fact about that reader rather than about
editable elements.

**Six variants were driven in Safari with no terminal in the path** (the probe is
`ui/probes/element_probe.html`'s macOS counterpart, to be committed with this entry).
The shipped shape — `contenteditable`, keys prevented, `textContent` written — is silent
for typing, deleting, completion *and* the arrows. The same field with nothing prevented
gives full native echo, including the deleted character, which is the control proving
VoiceOver's typing feedback is on. **Applying the far end's answer as a real editing
command** (`execCommand insertText`, backward `delete`) so WebKit sees an ordinary edit
is **also silent** — the obvious fix, measured and refused. A live region fed the whole
row says `e`, `ec`, `ech`, which is decision 3's verbosity confirmed on a second reader.
A live region fed **only what changed** says `e`, `c`, `h`, then `o` for the completion:
one utterance per keystroke, about half a second behind the key.

**So the deliverable is an announcement channel that exists on macOS and nowhere else**,
fed the delta — the character that arrived, the character that went, what a completion
added — and *not* fed on caret-only moves, which VoiceOver already speaks and which would
double. On Windows nothing changes: feeding one there would say everything twice, which
decision 3 measured before this entry existed.

**That amends spec 28.** "The mode uses no live region at all" is true of NVDA and false
here, so the amendment rides in this entry's PR rather than being worked around quietly.
What it does **not** touch is DESIGN's rule that Acter detects no screen reader: the
switch is the operating system, which the frontend already asks the backend for
(`shell.platform()`, used by `applyPlatformText` since M3).

**How the two behaviours are selected** — the shape this lane has used since M1, and the
reason it is written down here rather than decided at implementation time. `FarEndFieldView`
is already a port. The macOS behaviour is a **decorator** implementing that port and
wrapping `FarEndFieldDom`, adding only the announcement; the composition root picks the
wrapped one when the platform answers `macos` and the bare one otherwise. No `#[cfg]`, no
branch in a controller, no second frontend, and both adapters are unit-testable on any
machine — which is the property `offered(os)` and `system_menu(os)` were given for exactly
this reason.

**Readers vary, and the platform is a proxy for the reader rather than the answer.**
Raised by the user 2026-09-03, and it is the right objection: what actually differs is not
macOS and Windows, it is what a *reader* does with a change the application makes, and
Windows has more than one reader. So the thing to name is the **duty**, not the platform:
either the reader speaks what was typed and Acter must stay quiet, or the reader speaks
nothing Acter does not say and Acter owes the words. NVDA is the first kind, VoiceOver the
second, and JAWS, Narrator and Orca have not been measured.

**Neither branch can be detected, and that is the load-bearing fact.** No platform offers
"which reader is running and what will it announce": Windows' `SPI_GETSCREENREADER` answers
that *something* is listening, macOS answers whether the accessibility API is on, and
neither says what the reader will do with a programmatic text change. Feature detection
cannot help either — there is no way to write a row and ask whether it was spoken. So a
detected answer would be a guess wearing an API's clothes, and DESIGN's rule that Acter
detects no screen reader stands for a reason rather than by habit.

**What is proposed instead, and it needs approving before the spec is written**: the duty
is a value with a per-platform default and a setting that overrides it. `echo_duty(os,
preference)` is a pure policy in the core, asserted on any machine, exactly as
`offered(os)` and `system_menu(os)` are; the frontend asks for it once and the composition
root wraps the far-end field or does not. The default is the reader that platform's users
overwhelmingly run — NVDA on Windows, VoiceOver on macOS — and the setting is what a
listener on JAWS reaches for if this lands wrong for them.

**The failure the setting exists to prevent is double speech**, which is worse than
silence: a reader that speaks typed characters, plus an Acter that also speaks them, says
every keystroke twice and there is no way to hear past it. That is decision 3's finding on
NVDA and it is why the wrong default must be reachable and reversible rather than merely
unlikely — so the setting has to be findable by ear, which makes the Help topic part of
this entry rather than a follow-up.

**One field, not a profile of them.** The temptation is a record of duties — echo,
deletion, completion, caret — one row per reader. Only the echo duty has been measured to
vary, and this project's own rule is that a distinction nobody measured is a distinction
nobody should ship, so the value carries what was measured and grows a field when a
measurement earns it.

**What the entry still has to decide**: where the delta comes from. The adapter can diff
the row it holds against the row it is given, which needs no protocol change and is a
guess about nothing (it compares two strings it owns); or the domain, which knows which
key was pressed, can carry it in the event. Measure the first, because it is smaller, and
say so if the far end's redraws make it lie.

**What already works on macOS, so nobody measures it twice**: the connection sentence,
the once-only "Remote process keys" sentence, focus landing on the command line when a
session starts, a command's output, a failing command's verdict, the completing `Tab`, and
every caret key.
