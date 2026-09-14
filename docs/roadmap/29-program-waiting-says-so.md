# 29 — A program that is waiting says so.

Lane: keyboard-routing. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

A program that is waiting says so. Spec: none yet → specify first. **Agreed
2026-08-31**, and it is what makes 28 discoverable rather than a mode only its author
knows about.

The seam is this. A user types `gh pr create` in local-line mode, presses Enter, hears
some output, and then hears nothing, because the program is waiting for a keypress that
the edit field will never send. There is no event, no announcement, and a session that
has simply gone quiet — the failure shape DESIGN.md names as the one this product can
least afford. Nothing in 28 helps, because the user has no way to know that 28 is what
they need.

The signal needs no new machinery: output produced, then quiescence, with the command
not ended. It is the same clock the pacing policy already runs.

**Two candidate discriminators were measured on 2026-08-31, and one of them died.**
Cursor hiding is *not* evidence of waiting: `readline` brackets every one of its redraws
with `ESC [ ? 2 5 l` and `ESC [ ? 2 5 h`, so a shell editing a line looks exactly like a
widget drawing itself, and an idea that looked promising before it was tried is recorded
here so nobody tries it twice. Bracketed paste is more interesting and survives: `bash`
turns it on at every prompt, and `gh`'s selection prompt never touches it — so "settled,
and bracketed paste is off" is positive evidence that whatever holds the terminal is not
a shell waiting for a command line. That is a candidate, on two programs, and the spec
should widen the sample before it leans on it.

**A second and better discriminator, measured 2026-09-02, and it contradicts this
entry's premise.** `less -X` — the case DESIGN cites as the one where "Acter has no
signal at all" — turns on **application cursor keys** (`ESC[?1h`) while never touching
the alternate screen. So a program that wants raw keys on the primary screen can
announce itself, and the pager case is detectable after all. `gh pr create` does not set
it, so DECCKM separates the announced half of the semi-interactive population from the
silent half rather than covering all of it — but a hint that fires reliably for pagers
and readline-driven far ends is worth more than one resting on bracketed paste alone.
Both should be in the spec, and the sample should still be widened.

**The difficulty is false positives, and it is the whole entry.** A command that
*finished* also produces output and then goes quiet. Telling the two apart is exactly
the problem this project has met before from the other side: in an integrated session
the `D` marker says the command ended, and in an unintegrated one nothing does. So the
hint is cheap where markers reach and delicate where they do not, and the spec has to
say what it does in the unmarked case rather than quietly assuming the marked one.

**It depends on 22.8 and cannot be built before it.** In a real unintegrated session
`Pump::open` is never cleared, so "a command is running" is currently true forever, and
a hint gated on it would fire after every command in the session. 22.8 is what makes the
gate mean anything.

Two further constraints for the spec. It is said **once per command**, never repeated,
because a hint that nags is worse than one that is missed — the babble guard exists for
the same reason. And its wording is a plain sentence about what to do, not a diagnosis:
what the user needs to hear is that the program seems to be waiting for a keypress and
which key hands it the keyboard.
