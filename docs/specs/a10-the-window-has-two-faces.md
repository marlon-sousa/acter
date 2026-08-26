# A10 — the window has two faces, and the connected one is the terminal window

Roadmap entry 13.4, lane 1. Agreed in conversation 2026-08-26, immediately after A8 was
built and driven with NVDA. **Depends on B7** for the unconnected state and on **A8** for the
Connect dialog this window's button opens.

## What is wrong with the window A8 leaves

B7 decided that Acter starts unconnected and that the window says so. It says so into a
window that still holds **an empty results buffer and an edit field that can submit nothing**
— because those two are what the window has always had, and nothing had asked whether they
belonged there when there was no session behind them.

The user put it plainly on 2026-08-26: a window with no connection should exhibit neither.

They are not merely useless. A region holding nothing is a thing a listener arrows onto and
hears nothing useful from — measured on 2026-08-25, where the empty buffer read as a bare
letter `R` (roadmap 13.2) — and a field that can submit nothing is a control they have to
pass to reach the only thing that would help them. Two obstacles in front of the one action
available is the shape of an interface designed by looking at it rather than by listening
to it.

## Decisions

### 1. Two faces, and the connected one is the terminal window

The user's own name for it, and it is the right one: the **terminal window** is a results
buffer and an edit field, and it belongs to a session.

- **With a session**: the terminal window, and no Connect button.
- **With none**: a line saying it is not connected and a **Connect button**, and no edit
  field.

The heading, the menu bar and the status bar belong to the window rather than to either
face, and do not move.

### 2. The Connect button takes focus when the window opens

**A button rather than an instruction.** B7's window announced "not connected. Press F10 for
the Acter menu, then choose Connect" — a route to describe rather than a thing to do, and
describing a route is what this product exists to stop doing. One control, under focus, and
Enter connects.

The announcement changes with it, to `not connected. Choose Connect to start a shell`: it
names the control the listener is already on.

**The menu item stays.** A menu is where a user looks for a command they already know, and
the button is where a user who has just arrived finds it. They are the same action.

### 3. The buffer is in the document only while it has something in it

Not "hidden when there is no session" — **hidden when empty**, which is a different rule and
the right one. It appears with its first content and stays afterwards.

That single rule covers three cases: a fresh unconnected window has no buffer; a window that
has just connected has none until the shell speaks; and a window whose session has ended
keeps the one it filled.

**It also closes roadmap 13.2**, the empty region that read as a bare letter, by removing the
empty region rather than by explaining it.

### 4. When the far end goes away, the buffer stays and the edit field goes

The buffer is by then **the record of what happened**, and a user who typed `exit` by accident
must not lose it. The edit field has nothing left to submit to, so it goes, and the Connect
button comes back below the buffer.

This is why decision 3 is about emptiness rather than about sessions: a rule keyed to "is
there a session" would have taken the transcript away at the worst possible moment.

### 5. Focus is rescued, never stolen

Hiding the element focus is inside strands it on the document body, where a listener has
nothing under them and no obvious way back. So focus moves into whatever is now showing —
but **only if it was in what just went away, or nowhere at all**.

A user reading the buffer when their shell exits keeps their place. A user typing when it
exits is carried to the Connect button rather than dropped.

### 6. The terminal window is one thing in the document, because tabs will make it many

Phase 1 has one session and therefore one terminal window. The buffer and the edit field are
**grouped in the markup** rather than left as siblings of `<main>`, so the day a tab holds
each of them is a change in one place rather than an untangling.

The wrapper carries no role and no label: an unnamed `div` is invisible to a screen reader,
so grouping the two costs a listener nothing today and gives the tab panel somewhere to be
when it exists. It carries no `hidden` either — its two halves each know when they belong in
the document, and a wrapper whose children are both hidden is already nothing at all.

The rules above are then per-tab: a tab whose session ended keeps its buffer and loses its
edit field, and a window with no tabs at all is the Connect button.

### 7. What this supersedes

**B7 decision 3's third bullet**: "a line submitted while unconnected is answered, never
swallowed, and the edit field keeps the text". With no edit field there is nowhere to type
one. The refusal survives in the protocol — `SubmitAck::NotConnected` — as the answer a
submission gets when it names a session that has since ended, which is a race rather than
something a user can do on purpose, and the controller's guard stays for exactly that.

### 8. Two things the reader found, and both changed the design

**Focus placed while the page is loading does not take the browse cursor with it.** The
window opened with the Connect button focused and announced as such, and the first Enter
opened the *menu bar* — because NVDA reads a document in browse mode, where Enter acts on its
own cursor, and that cursor was still wherever NVDA's initial read of the document had left
it. Measured with NVDA 2026.1.1 on 2026-08-26.

Two changes came out of it, and both were needed:

- **A startup hold before the first focus placement**, the same shape as the announcer's own
  and with the same honesty: the value is a starting point rather than a measurement. It
  moved focus onto the button reliably — and Enter still did nothing, because the cursor by
  then sat on the status bar.
- **`role="application"` around the button**, which is A7's measured device applied to a new
  place: it says the keys in here belong to the widget, so focus mode applies and Enter
  presses what is focused. It wraps the button alone; the line above it stays ordinary
  browsable text, which is what a listener reading the window top to bottom needs to meet.

With both, the first Enter after the window opens reaches the dialog. Measured.

**The menu bar and the dialogs returned focus to the edit field by name**, which was right
while there was always one. Escape out of the menu bar in an unconnected window focused a
hidden input — which does nothing at all — and left the listener stranded on the menu item
they had just closed. They return to the window now, which puts focus wherever the face that
is showing keeps it.

## Files touched

- `ui/src/views/main_window.html` — the terminal wrapper, the not-connected block, and the
  two `hidden` attributes that are the initial state.
- `ui/src/ports/window_view.ts`, `ui/src/adapters/window_chrome.ts` — `showTerminal`, and the
  focus rescue. The adapter's arguments become a named object, because six positional
  elements is a signature nobody can read.
- `ui/src/adapters/buffer.ts`, `ui/test/adapters/buffer.test.ts` — the empty rule.
- `ui/src/controllers/app.ts` — the face follows the session; the disconnect path; the
  reworded pinned string.
- `ui/src/main.ts` — the button's click, and focus stops being placed here.
- `ui/src/styles.css` — the empty state.
- `e2e/test/specs/connect.spec.ts` — the faces in the real window.
- `docs/DESIGN.md` — the decision.

## Definition of done

- [x] An unconnected window holds no results buffer and no edit field, and focus is on the
      Connect button when it opens.
- [x] The Connect button opens the same dialog the menu item does.
- [x] Connecting brings the terminal window up before the attach, so nothing arrives at a
      window of the wrong shape.
- [x] The buffer appears with its first content and not before.
- [x] A far end that goes away leaves the buffer and takes the edit field; focus is rescued
      only if it was in what went away.
- [x] vitest over all of it, and an E2E spec over the faces in the real window.
- [x] `cargo fmt`, `cargo clippy --workspace --all-targets`, workspace tests, vitest and the
      E2E suite all clean.

## Accessibility checklist for the PR body

Agent-observable through the screen-readers bridge, `user` persona:

- [ ] An unconnected window reads as a heading, "not connected", and a Connect button, with
      focus on the button — and nothing else between them.
- [ ] Enter on that button opens the Connect dialog.
- [ ] After connecting, the edit field has focus and the Connect button is gone.
- [ ] The buffer is not reachable before the first output, and is reachable after it.
- [ ] Typing `exit` leaves the buffer readable, takes the edit field away, and moves focus to
      the Connect button rather than dropping it.
