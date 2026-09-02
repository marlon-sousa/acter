# 28 — the keyboard goes to the far end, and the row it redraws is what you hear

Roadmap entry **28**, with **23.5** folded into it. Agreed in conversation 2026-09-02,
after four measurement runs made that day: an element probe on a real NVDA through the
screen-readers bridge, and three captures on a real pseudoconsole through Acter's own
engine (rig: `crates/acter-transports/examples/capture.rs`).

It is one PR rather than three because the pieces are not separable by a listener. Routing
keys to the far end with no way to hear the answer is not a feature; hearing the answer
while the transcript grows a junk line per arrow press is not one either.

Depends on **B3** for identified lines and revisions, **B4.9** for the positional echo rule
and the anchored row, **B5.2** for a shell's measured end-of-input answer, and **B6** for
the keystroke seam — `send_key`, and the binding table behind the port.

## What is true today

- **The frontend reports one keystroke.** `isReportable` in `ui/src/adapters/keyboard.ts`
  answers true for `Ctrl+C` and nothing else, so `Ctrl+D` never leaves the page — which is
  the whole of roadmap 23.5.
- **`Key` has one variant**, `Char(char)`. Named keys were deferred to the entry that
  needed them (spec B6, decision 6). This is that entry.
- **The binding table is a function of the keystroke alone**: `policies::keybindings` maps
  `Ctrl+C` to `Interrupt`, `Ctrl+D` to `Eof`, everything else to `None`.
- **A rewrite is dropped.** `Pump::due` answers `None` for `LineRevision::Rewritten`, so a
  redrawn row reaches neither speech nor the buffer. `SessionEvent::Output` carries
  `{ command_id, text }`, and `LineId` does not cross the wire at all.
- **The engine has no cursor.** `TerminalEngine` exposes `advance`, `screen`, `resize` and
  `take_replies`, and nothing that says where the far end's cursor is or which modes it
  turned on.
- **The edit field is a real `<input>` and it owns the line.** Nothing reaches the far end
  until Enter.

## What was measured, and where

### The element, on NVDA 2026.1.1 through the bridge, `user` persona, silent capture

A static page in Edge — the engine the app's WebView2 runs — with a field held permanently
empty, as this mode's field would be.

- **A plain `<input>` says "blank" before every arrow.** Right, left, up, down, Home and
  End each produced one utterance: "em branco".
- **Preventing the arrows changes nothing.** The identical field with `preventDefault` on
  every arrow said "blank" exactly as often. The caret in an empty field had nowhere to move
  in the first place, so the word comes from NVDA reading the line *after* a caret command,
  not from a caret that moved. **This answers the last open question under "Edit field
  ownership"**, and it answers it the other way from the guess.
- **Answering into a live region does not mask it**, politely or assertively: "em branco",
  then "cargo test --all" — two utterances, same millisecond, the blank first.
- **A focusable non-text element is silent**: a `<span tabindex="0" role="group">` said only
  what Acter put in the live region. **But so is typing into it** — three typed characters
  produced no speech, where the same three in an `<input>` were spoken as "a", "b", "c".
- **`role="textbox"` over no editable text is the worst of both**: NVDA reports it as
  read-only editable text, says "blank" on every arrow, and does not speak typed characters.
- **The shape that works is an ARIA text box whose text and caret are written by script.** A
  `<span contenteditable="true" role="textbox" aria-multiline="false">`, every key
  prevented, content and caret set from what the far end did:
  - landing on it: "V6 command line, edit, cargo test --all" — announced exactly as an edit
    field, which is what `role="textbox"` buys; without it NVDA says "section, multiline,
    editable";
  - right arrow with the caret placed at column 1: **"a"**, the character at the far end's
    cursor;
  - up arrow with the text replaced by `exit`: **"exit"**, the rewritten row;
  - typing `y`: **"y"**, because the element is editable;
  - a row a key emptied: **"blank"**; the caret past the last character: **"blank"**. NVDA's
    own words, in a vocabulary its users already have.

### PSReadLine's completion menu — the third prompt-driven sample

`pwsh` 7.6.5, `Get-C` then Tab bound to `MenuComplete`, then right, right, down.

- No alternate screen, no DECCKM, no bracketed paste. `pwsh` asks for win32 input mode
  (`ESC[?9001h`) at startup, and plain VT arrows drove it correctly regardless.
- **The selection is drawn in colour alone** — `ESC[30m` and `ESC[47m` around the item, with
  no marker character anywhere. `gh`'s `>` is that program's choice rather than a
  convention, so a rule naming a marker would hear nothing here.
- **But the answer is on the anchored row.** Each arrow rewrites the command line itself:
  `PS C:\Users\marlo> Get-CIPolicyInfo`, then `…Get-CertificateAutoEnrollmentPolicy`, then
  `…Get-CertificateEnrollmentPolicyServer`. One item per press, which is what a listener
  wants, and the anchor rule already yields it.
- **The repaint is large and must route nothing.** The first arrow produced eleven line
  items: the command line rewritten, and ten menu rows blanked. A rule sending "most of the
  screen changed" to interactive mode would mis-route ordinary Tab completion in PowerShell.
- Tab on an unambiguous prefix appended `ldItem` — the same shape as `gh`'s `o `, and
  nothing a listener could use without the anchor.

### What a key has to be spelled as, on three far ends

The same script against `bash` under WSL, `pwsh`, and `cmd.exe`: type a line, move with each
candidate spelling, type a marker character, and read where the marker landed.

- **Backspace must be `0x7f`, and `0x08` is a defect.** In `readline` both delete one
  character. In **PSReadLine and in `cmd.exe`, `0x08` deletes the previous *word*** — a line
  went from `BAhello worldECD` to `BAhello CD` on one press — while `0x7f` deletes one
  character on all three. This is the silent-garbage failure entry 28 predicted for the
  arrows and did not find there: it is real, and it is Backspace.
- **Home and End have two spellings each, and every far end took both**: `ESC[H` and
  `ESC[1~`, `ESC[F` and `ESC[4~`.
- **Delete is `ESC[3~`** on all three; left and right are `ESC[D` and `ESC[C`.
- **Ctrl+U is the far end's business.** `readline` and PSReadLine clear the line; `cmd.exe`
  inserts a literal `^U` into it. So "the row a key emptied" is something Acter reports and
  never promises.

## Decisions

### 1. The user toggles it, the frontend owns the key, the domain holds the state

`Ctrl+Shift+K` stays layer 1 and is never reported as a keystroke. It calls a new invoke,
`set_line_owner(session, LineOwner)`, with `LineOwner::Local` and `LineOwner::FarEnd`.

The domain holds the state because the domain needs it: which bytes a key becomes, whether
Enter opens a block, and which row goes in front of the listener all depend on it. Holding
it in the frontend would put a second binding table there, which is what the seam exists to
prevent (spec B6, decision 4).

The announcement is the frontend's, pinned beside every other announced string, and says
what is gained and lost rather than naming a mode:

- on: "The program gets your keys now. Its own history and completion — Acter's are off."
- off: "Acter gets your keys again. History and completion are back."

### 2. The far-end line is an ARIA text box whose text and caret Acter writes

While the mode is on, keys go to a `<span contenteditable="true" role="textbox"
aria-multiline="false">` labelled as the command line, and the `<input>` that owns the local
line does not hold focus. Acter writes two things into it and nothing else: the anchored
row's text, and the caret at the far end's cursor column. Every key is prevented, and no
character is ever inserted locally.

**This reverses DESIGN's "no region, no key sink, no new element", and the amendment rides
in this PR.** That decision rested on a guess this entry was told to measure — that
preventing the arrows would remove the "blank" — and the measurement says it does not. It
also rested on `role="application"`, whose cost is real and recorded in
`ui/src/adapters/readable_field.ts`, and which this shape does not use: no region, the
document stays browsable, and the handoff into focus mode is still the user's own
NVDA+Space.

**It also reverses the older invariant that the field never renders remote state**, and that
reversal is narrower than it sounds. The invariant was written about the *local* line, where
a field rendering remote echo would race the user's own typing and hand them a caret nobody
owns. Here the far end owns the line by definition — there is no local line to race — and
the element is written when the row settles on the quiescence clock rather than per byte.
What the invariant forbade cannot arise in the state it does not describe.

### 3. In this mode the reader does the speaking, and Acter invents no strings

Because the element holds the row and the caret, NVDA answers every key out of its own
text-box behaviour: the row when the row changed, the character at the caret when only the
cursor moved, the character typed when the user typed, and "blank" for an emptied row or a
caret past the end. **The live region is not used by this mode at all**, and the two strings
this spec was going to invent — one for a row a key emptied, one for end of line — are
deleted rather than decided.

What a listener hears is therefore identical in kind to what they hear in every other text
box on Windows. That is the strongest argument for this shape, and the reason it is worth a
reversal.

### 4. Keys become bytes in a policy beside the grid, never in the frontend

`Key` gains named variants — `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `Tab`, `Enter`,
`Backspace`, `Delete`, `Escape` — and nothing speculative. A new pure policy,
`policies::key_bytes`, maps a `KeyPress` plus the far end's current modes to bytes, with the
table as measured:

- Backspace is `0x7f`. **Never `0x08`**, which eats a word on two of the three far ends
  measured; `0x08` is what `Ctrl+Backspace` becomes.
- Home is `ESC[H`, End is `ESC[F`, Delete is `ESC[3~`, left and right are `ESC[D` and
  `ESC[C`, up and down are `ESC[A` and `ESC[B` — or the `ESC O` forms when the far end has
  turned on application cursor keys.
- `Ctrl` plus a letter is the control byte, which is how `Ctrl+C`, `Ctrl+D` and `Ctrl+U`
  reach the far end in this mode with no special case at all: `0x03`, `0x04`, `0x15`.
- `Alt` plus a key is that key's bytes with `ESC` in front.

The frontend sends the named key. It has never been able to know which spelling is right,
and after this it never has to.

### 5. `TerminalEngine` grows a cursor and a modes accessor

`fn cursor(&self) -> Cursor` — column, row, and whether it is visible — and `fn modes(&self)
-> TerminalModes` — application cursor keys, bracketed paste.

The cursor is what places the caret in the element, which is why left, right, Home and End
need no speech path: they rewrite no text and are invisible to every diff rule, and the
answer to them is a caret rather than a sentence. Visibility earns its place because `gh`
hides the cursor and parks it off the list, and a caret must not be placed from a cursor the
far end is not using.

`modes()` is one accessor for two facts the emulator already tracks, and it settles both
encoding questions at once: which arrow spelling to send, and whether a paste may be
bracketed.

### 6. Which row goes in the element: two steps, and no per-program knowledge

After a key Acter sent, once the row settles on the quiescence clock the pacing policy
already computes:

1. **If the anchored row changed, that is the answer**, from the anchor column onward.
   Readline's history recall and Tab completion, PSReadLine's completion menu, and every far
   end that has a command line.
2. **Otherwise, among the rows that changed, take the one that gained non-whitespace
   content.** `gh`'s selection prompt, where the cursor is hidden and parked below the list
   and the anchored row never changes.
3. **Otherwise, if the cursor is visible and its column moved**, only the caret moves.
4. **Otherwise nothing happens.**

Both kinds of change count in step 1, an append and a rewrite: the first up arrow at a fresh
prompt appends rather than rewriting (measured 2026-08-31), and a rule keyed on rewrites
alone would be silent on the commonest press of the commonest key.

Row count routes nothing. PSReadLine's first arrow changed eleven rows and is ordinary line
editing; the alternate screen is the only boundary that means anything, and it belongs to
phase 2.

### 7. Enter uses the anchored row as the echo, and an empty row earns no heading

At the instant Enter is sent, the anchored row *is* the far end's echo — every character on
it went down the wire and came back. The evidence test B4.9 already uses therefore arrives
one step earlier and needs no new machinery:

- anchored row non-empty: open the block, and that text is the heading;
- anchored row empty: no block and no heading.

The second branch disposes of the widget case for free: answering a `gh` prompt with arrows
leaves an empty anchored row and earns no heading, because answering a question is not
running a command.

**The filter edge is accepted rather than guessed at.** A user who types characters to filter
a `gh` prompt leaves a non-empty anchored row and gets a heading naming their filter. That is
not a leak: the far end echoed those characters, so they are on screen and in the transcript
whatever Acter does, and the alternative is a rule that guesses which typed text was a
command — the guess this project has refused twice.

### 8. The buffer applies revisions by id, blanks included, and keeps nothing else

`LineId` becomes a protocol type and `SessionEvent::Output` carries it with the revision, so
the frontend assigns or appends by id instead of appending text. `Pump::due` stops dropping
`Rewritten`: it stops being a speech question and becomes a rendering one, which is DESIGN's
separate-paths decision unchanged — the buffer applies all three revisions, and speech takes
what decision 3 gives it.

This is what makes the transcript honest in this mode. Without it, arrowing a history list
appends a line per press, and a `gh` prompt answered with Cancel leaves its three option rows
behind — where the far end itself blanked them and rewrote the question row as `? Where
should we push the '…' branch? Cancel`. The far end writes its own record; Acter keeps that
and nothing else.

### 9. Ctrl+D reaches the far end, and says something when it cannot

In far-end-line mode `Ctrl+D` is `0x04` on the wire like any other control byte: no intent,
no adapter answer, and aimed at the far end rather than at the shell Acter spawned — which is
what makes it right inside an `ssh`, where today's `Eof` would send PowerShell's `exit` into
the wrong shell.

In local-line mode `isReportable` grows `Ctrl+D`, which is the whole of 23.5's fix, and the
three answers get three sentences. `KeyAck` gains `Unsupported`, because "bound, and this far
end has no measured answer" and "bound, and nothing is listening" are different things to
say:

- the far end took it and the session ended: **no new string**, the connection sentence
  already says so;
- `Unsupported`: "This shell has no key for end of input. Type exit and press Enter."
- `NothingToActOn`: "This session has already ended."

The frontend words the answer to the key it sent. It still does not decide what the key
means, and the binding table stays behind the port.

### 10. A paste is bracketed when the far end asked for it, and never otherwise

Pasting into far-end-line mode wraps the text in `ESC[200~` and `ESC[201~` when
`modes().bracketed_paste` is on, and sends it bare when it is not. `bash` turns it on at
every prompt and `gh` never touches it, so both branches occur in ordinary use. Sending the
wrapper unconditionally puts its bytes into a far end that never asked; never sending it runs
each pasted line as it arrives, which is data loss rather than noise.

### 11. History stays out, and is a decision rather than a leftover

Acter now knows every command the user ran in this mode, since decision 7 reads the echoed
line. Keeping them out of Acter's history is therefore a choice, and it is the same one
DESIGN records: this history is offered as recall and completion *on this machine*, and the
complaint that produced this whole mode is that it already holds lines typed at every far end
the profile reached. If it is ever revisited, the shape is history keyed to the far end, never
one pool.

## Files touched

- `docs/DESIGN.md` — amend "Edit field ownership" and the open question under it: the element
  decision is reversed, with the measurement, and the "blank" probe is answered.
- `docs/ROADMAP.md` — entry 28 to Done with its spec; 23.5 to Done, folded here; entry 30
  reduced to its measurement note, since decision 6 covers it.
- `crates/acter-core/src/entities/protocol_commands.rs` — `Key`'s named variants,
  `KeyAck::Unsupported`, `LineOwner`.
- `crates/acter-core/src/entities/terminal_item.rs` — `LineId` and `LineRevision` become
  protocol types.
- `crates/acter-core/src/entities/protocol_events.rs` — `Output` carries the line and its
  revision.
- `crates/acter-core/src/policies/key_bytes.rs` — new: the measured table.
- `crates/acter-core/src/policies/keybindings.rs` — the table takes the owner.
- `crates/acter-core/src/policies/far_end_row.rs` — new: decision 6's two steps, as a pure
  function over the revisions of one settled batch.
- `crates/acter-core/src/ports/driven/terminal_engine.rs` — `cursor`, `modes`, and their
  value types.
- `crates/acter-term/` — the adapter answering both from the grid it already keeps.
- `crates/acter-core/src/ports/driving/session_api.rs` and
  `crates/acter-core/src/services/session.rs` — `set_line_owner`, the far-end key path,
  Enter's anchored-row rule, and `due` no longer dropping rewrites.
- `crates/acter-app/src/` — the invoke.
- `ui/src/adapters/far_end_field.ts` — new: the ARIA text box, its text and its caret.
- `ui/src/adapters/keyboard.ts` — `Ctrl+D`, `Ctrl+Shift+K`, and the far-end routing.
- `ui/src/adapters/buffer.ts` — apply revisions by id.
- `ui/src/controllers/app.ts` — the toggle and the three `Ctrl+D` sentences.
- `crates/acter-transports/examples/capture.rs` — the key spellings this entry measured with,
  kept because the next entry will want them.

## Amendments made while implementing

Five, all small, all riding in the PR that implements this. Nothing here reverses a decision;
each is a place where the decision as written did not say enough to build from.

### A. The row and the caret reach the frontend as `SessionEvent::FarEndLine`

Decision 2 says Acter writes the anchored row's text and the far end's cursor column into the
element and says nothing about how they get there. They get there as one event carrying
`text: Option<String>` and `caret: u32`. `None` is "nothing was redrawn and only the caret
moved", which is the whole of what left, right, Home and End do — and it has to be distinct
from `Some(the same row)`, because writing a row back unchanged is a text change the reader
announces as one.

### B. The anchor is never taken from a cursor the far end is not showing

Decision 6 has the anchored row as step 1 and `gh`'s content rule as step 2, and decision 5
says a caret must not be placed from a cursor the far end is not using. Building it showed
those are the same rule read twice: if the anchor is taken wherever the cursor happens to be
when a widget has the screen, the anchored row *is* the widget's row, step 1 fires on it, and
step 2 never runs — so `gh`'s option rows would be read from an arbitrary column instead of
one option per press.

So the anchor is taken only from a visible cursor. `gh` hides it before drawing and parks it
below the list, so the anchor stays where the last prompt put it — a row that does not change
while the widget is up — and the content rule gets the press. This is decision 5's reasoning
applied one step earlier than it was written, and it is what makes decision 6's two steps
land on different rows.

The anchor is taken at three moments and no others: when the user hands the line over, at
every settling where no key is outstanding, and at the first settling after Enter. It is
never re-taken while a key is in flight, which is what stops typing from dragging it along
the row.

### C. `Cursor` carries the row as well as the column, and the row is read

Decision 5 names "column, row, and whether it is visible" and gives a reason only for the
column and the visibility. The row earns its place in step 3: "the cursor moved" and "the
cursor moved *along the row it was on*" are different things, and only the second is a caret
moving through a line the user is editing. Without it, a far end that moved its cursor
somewhere else entirely would place the caret at a column in a row nobody is on.

### D. Pasting is its own invoke, and `Escape` stays the far end's

Decision 10 requires bracketed paste to be honoured, which needs a path: `SessionApi::paste`,
because a paste cannot be a run of `send_key` calls — only the domain knows whether the far
end asked for the wrapper, and each pasted line would otherwise be run as it arrived.

And `Escape`: DESIGN's keystroke map calls it contextual, and in local-line mode it returns
focus from the buffer to the edit field. While the far end owns the line it is the far end's —
it leaves insert mode in `vi`, closes a completion menu in `readline`, cancels a `gh` prompt —
so the frontend stops claiming it. The local `<input>` is also taken out of the document
rather than merely losing focus, because two edit fields where only one does anything is the
noise A10 took the field away to avoid.

### E. `LineId` is exported to TypeScript by naming a narrower integer to specta

`specta-typescript` refuses to export `u64` at all, to stop a caller silently losing precision
in a JSON number. JSON is exactly what this crosses on, so serde already writes a number and
the TypeScript describing it is `number` whichever integer is named; the type carries
`#[specta(type = u32)]`, which is a statement about the exported shape and not about the id.
The two alternatives were narrowing a Decided domain type to suit a generator, and giving the
domain crate a dependency on the frontend's TypeScript exporter.

### F. The far-end line has its own clock, because decision 6's "once the row settles" was the transcript's

Decision 6 says the answer goes out once the batch settles, and did not say on which clock.
The implementation used `quiescence`, which is 500 ms and is DESIGN's number for turning
output into a chunk a listener is read. That was wrong, and roadmap 28.1 is what it cost: a
listener heard every key answered with the state before it.

**The window is the screen reader's and is not ours.** NVDA does not answer an arrow key from
the field as it stands. `EditableText._caretMovementScriptHelper` takes a bookmark of the
caret, sends the key on, and polls every 10 ms until the caret moves or `caretMoveTimeoutMs`
elapses — 100 ms by default, and the user's to raise to 2000. Only then does it speak, and on
timeout it speaks the caret that did not move: a late answer is not silence, it is the
previous answer said again. This is also how NVDA treats a terminal — `behaviors.Terminal` is
`LiveText` plus `EditableText`, and `WinConsoleUIA` raises the poll by 1.5x because "on older
consoles, the caret can take a while to move".

**The far end is not the slow part.** Measured 2026-09-02 with
`acter-transports/examples/latency.rs`: `bash` under WSL answers left in 1 ms, Home in 0 ms,
up in 3 ms, Backspace in 4 ms; Windows PowerShell all four in 0 ms; `cmd.exe` 0 to 1 ms —
each in a single batch, first change and settled at the same instant.

So `PacingConfig` gains `far_end_settle`, default 30 ms, and the pump uses it for a settling a
key is outstanding for. **It is a coalescing gap, not a latency budget**: it is added on top
of the round trip on every endpoint, so it is the smallest number that still holds a redraw
that arrived in pieces together, never the largest that fits in the window. What varies with
the endpoint is the round trip, and no number here can absorb it — the knob for that is the
reader's own `caretMoveTimeoutMs`, which is where it belongs and which NVDA itself reaches for
on slow consoles.

**A settling with no key outstanding keeps `quiescence`.** It only moves the anchor, nobody is
waiting on it, and running it at 30 ms would re-anchor and rewrite the field through every
quiet gap in a command's output.

**And so does the settling after a submission**, which is the one case that looks like the
first and belongs to the second (roadmap 28.5, found on the reader while re-running this
spec's own checklist). Enter leaves a key outstanding like any other, but its answer is not a
caret anybody is polling for: it is the far end running a command and drawing its next prompt,
and that settling is where the anchor is taken. Thirty milliseconds catches a far end
part-way through drawing — the prompt is on the row, the cursor has not reached the end of it
— and the anchor lands at column zero. Nothing is heard at the time; it goes wrong at the
*next* submission, which heads its block with everything from column zero. Observed: a heading
reading `marlon@splyt:/mnt/c/Users/marlo$ python3 /tmp/acter_menu.py` where the command alone
belonged. So the short clock is for `watching && !awaiting_prompt`.

This reverses nothing. Decisions 2 and 3 stand — the element is an ARIA text box, there is no
live region and no `role="application"` — and the row and caret the field held were correct
throughout the NVDA pass; `nvda+uparrow` re-read them accurately every time. Only *when* was
wrong.

**One thing the measurement found that the pass could not.** Left, right, Home and End rewrite
no line at all: the far end only repositions the cursor, and the engine reports no line item
for any of them. Decision 6's third step is what answers them, and it is load-bearing — any
future change keying this path off changed lines alone would silently lose the four commonest
navigation keys.

### H. Tab completion is not spoken, and the reason is not timing

Measured on the reader after the clock was fixed: every caret key now speaks the far end's
answer on the press, and **Tab still says nothing at all**. The completion itself is right —
`ech` became `echo ` in the field, confirmed on demand — so this is not decision 6 failing.

**NVDA speaks for a fixed set of keys, and Tab is not in it.** The poll-then-speak behaviour
amendment F relies on lives in `EditableText`'s caret-movement scripts, and its `__gestures`
table binds the arrows, `home`, `end`, the page keys, Enter and Backspace. Tab is not bound
there: in focus mode Tab means "announce the newly focused object", and the field prevents it,
so focus does not move and there is nothing for NVDA to say.

**A real terminal gets this from somewhere else.** `NVDAObjects.behaviors.Terminal` is
`LiveText` *plus* `EditableText`: the caret scripts answer the arrows, and `LiveText` monitors
the object's text and speaks what changed, which is what carries Tab completion, command
output and everything else no keystroke asked for. Acter's field is an ARIA text box, so it
has the first half and not the second.

That makes the choice a real one rather than a bug to fix, and it is **roadmap 28.4**, left
open deliberately: give this path the web's equivalent of `LiveText` — a live region carrying
only what the reader would otherwise not say, which is a narrower thing than the live region
decision 3 deleted and would not double-speak, because these are exactly the keys that produce
no reader speech; or accept that a completion is applied silently and read on the next
keystroke or on demand; or re-measure the element, since a role with an autocomplete contract
announces exactly this and would be a second element probe of the kind decision 2 already
turned on. **Do not guess between them.**

### G. Checklist item 7 asks for a prompt that creates nothing

The item as written says to answer a `gh repo create` selection prompt. Answering it creates a
repository, so it was not run in 28's pass. What it is actually checking is that a submission
at the far end's own prompt leaves the far end's one-line record and no Acter heading, and a
`bash` `select` loop puts exactly that in front of a listener while creating nothing.

What the item now names is `crates/acter-transports/examples/acter_menu.py`, kept with the
rigs: an inline selection prompt drawn the way `gh` draws one — options below the question,
the cursor hidden for the whole prompt, the option rows rewritten in place on every arrow, and
no alternate screen, which is a different path. It creates nothing, so the item can be run to
the end, which is what `gh repo create` never allowed.

## Definition of done

- Every named key becomes the measured bytes, with a unit test per row of the table and one
  that pins Backspace as `0x7f` naming why.
- The two-step row rule is a pure function with tests built from the captured transcripts:
  readline's recall and Tab, PSReadLine's menu (the anchored row wins over ten blanked rows),
  and `gh`'s two rows (the row that gained content wins when the anchored row did not change).
- Enter with a non-empty anchored row opens a block headed by it; with an empty one, no block
  and no heading. Tested both ways.
- The buffer renders a rewrite in place and a blank as a blank, with a test that arrowing a
  three-item list leaves three lines rather than nine.
- `Ctrl+D` leaves the page in local-line mode and is answered by one of the three sentences.
- Toggling the mode moves focus to the far-end field and back, and announces the sentence.
- `cargo check`, `cargo test`, `cargo clippy` and the frontend tests are green.

## Accessibility checklist (PR body)

One checkbox each, findings written inline on the unchecked item, naming the reader version
and the capture mode, and saying which items were agent-observed and which are the human's:

- [ ] Toggling into the mode announces what is gained and lost, once.
- [ ] Up arrow at a real `bash` over `ssh` speaks the recalled line, once, without the prompt.
- [ ] Tab completion speaks the completed line rather than the characters typed.
- [ ] Left and right arrows speak the character at the far end's cursor.
- [ ] Backspace deletes one character at `bash`, at `pwsh` and at `cmd`, and the row is spoken
      as it stands.
- [ ] Arrowing a `gh` selection prompt speaks one option per press.
- [ ] Answering that prompt leaves the far end's own one-line record in the buffer, and no
      heading.
- [ ] A command submitted in the mode opens a block headed by the line the far end echoed.
- [ ] Toggling back restores Acter's history and completion, and says so.
- [ ] `Ctrl+D` in local-line mode says one of its three sentences (human: the case where the
      session ends).
