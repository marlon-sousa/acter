# A13 — What a session can tell you, and where that is explained

Roadmap entry 13.7. Agreed in conversation 2026-08-27, from the user's question during B6.2's
accessibility pass: *"'shell integration unavailable, output will not be read' — is this still
true? Because prompts are now read."* Depends on **A7** for the menu bar and the modal-dialog
pattern, and on **A10** for where focus returns to.

## The sentence was false, and had been for five entries

`integrationUnavailableMessage` says:

> shell integration unavailable, output will not be read automatically; review it in the buffer

Output **is** read automatically. It has been since **B4.4** (roadmap 22.4, Done), whose spec
reversed DESIGN's "no auto-read" in as many words: *"What stays true is the rest of case 2: no
exit code, no verdict, and the patience announcement still fires. Only the silence goes."* The
policy agrees — `Integration` is not consulted anywhere in `policies/autoread.rs`, so an
unintegrated session's output goes through exactly the pacing path an integrated one's does.
The comment above the string says "nothing is read aloud" and is wrong the same way.

**B6.2 did not break this. It made the lie audible**, because the prompt is now read too: a
listener hears "output will not be read automatically" and then immediately hears output.

The user's second finding is the one that decides this entry's shape. Offered three corrected
sentences, all of them accurate, the reply was: *"Are these sentences what will be presented to
the end user? If so not even I myself can understand."* Every one of them used this project's
vocabulary — "shell integration", "verdict", "exit code" — to a listener who has none of it, and
the author of the product could not read them. That is the defect, and it is larger than the
false clause.

## Decisions

### 1. The sentence says what you keep and what you lose, in words a user owns

> You will hear what commands print here, but not whether they worked. Press F1 for help.

Chosen by the user from three plain candidates, 2026-08-27. Three things it does deliberately.

It **names what still works first**, because the fear the old sentence created was that output
had been withheld — and a listener who believes that goes looking in the buffer for text that
was already read to them.

It **names exactly one missing thing**, in the words a person would use about a command rather
than the words a terminal uses about itself.

It **ends with a way to find out more**, which is the half that did not exist and which the rest
of this spec builds.

### 2. Help is a place, not a longer sentence

An announcement is heard once, cannot be re-read, and competes with the output arriving behind
it. Anything that needs explaining rather than stating belongs somewhere a listener can go back
to at their own pace, which is what the user asked for. So the pointer is real: a Help menu, an
F1 that works anywhere in the window, and a modal dialog with the explanation in it.

### 3. F1, and it belongs with the window's own keys

F1 is the platform's "explain this", it is unclaimed here, and it is one keystroke with nothing
to disambiguate — the same argument A7 made for F10. It listens on the document alongside F6 and
Escape in `adapters/keyboard.ts`, which is where a key that belongs to the whole window lives.

The menu item exists too, for A7's reason: a menu is where a user looks for a command they know
the name of.

### 4. **The Help dialog is not an application region**, and that is the decision

Every dialog B9 added carries `role="application"`, so that arrows and Enter reach the widget
rather than a browse-mode cursor. This one must do the opposite. What is inside it is *prose to
be read*, and the Connect dialog's own comment already records the cost: "inside an application
region prose cannot be arrowed". A help topic a listener cannot arrow through line by line is
not help.

So it follows the About dialog, which has no such wrapper: a plain modal `<dialog>`, browse mode,
headings and paragraphs, `keepTabInside` for the Tab the platform does not cycle, and focus
returned to whatever the window is showing when it closes.

### 5. Opening it says one line, not the whole topic

**Added after the pass, which found the opposite shipping.** With no description of its own,
NVDA announced the dialog's name and then read the entire body in a single utterance — six
paragraphs, about two hundred words, **with the headings left out of it**, because a reader
with nothing else to speak falls back to the content. So the one read a listener gets for free
was the wall of prose, and the part it dropped was the structure decision 4 exists to provide.

An explicit `aria-describedby` pointing at one short line fixes it, and it is not a new idea
here: B9's host-key dialog already does exactly this, and this same pass watched it announce
one paragraph and leave the fingerprint to be found on its own tab stop. The line names what is
here and how to move through it — *"Three short sections about what you hear when you run a
command. Use your reader's heading key to move between them."*

### 6. The topic explains the difference by what a listener hears, not by what a shell emits

No "OSC 133", no "markers", no "integration" as a thing to understand. The topic answers three
questions in a listener's own terms: what you always get, what you sometimes do not get, and
which sessions are which. Headings so it can be skimmed with `h`.

### 7. What this does not touch

The connection sentence still says "…bash, with no shell integration set up on this host". It is
the same vocabulary and the same objection applies to it, but it was decided and measured in B9
and 27.5 barely a day ago, and rewording it is a change to a *different* sentence with its own
reasons. Filed as roadmap 13.8 rather than done quietly here.

## Amendment from the user's own pass, 2026-08-30: a fourth section, and a way in from the control

**Reported: a Help button after the "Let Acter set this session up…" checkbox, leading to a
place that explains what it is and how it works.** Decision 2 built exactly that place — a
topic a listener reads at their own pace instead of an announcement heard once — and gave it
one way in, F1, which answers "what am I hearing" from anywhere in the window. What it did
not answer is "what does this control do", asked while standing on the control.

Two things change, and neither touches decision 4: this is still the one dialog in the
product that is deliberately not an application region.

**A fourth section, "What setting a session up means."** In the register decision 5 settled
on: what Acter would run, what you get for it (a heading for each command, and being told
when one fails), what you keep without it (everything else — what you type runs, and what it
prints is read out), and that nothing is ever run without being shown to you first. The
summary line says four sections now instead of three.

**Help can be opened at a section.** `HelpDialog.open` takes an optional topic — the id of a
heading — and focus lands on that heading, so somebody who asks what a checkbox does arrives
at the answer rather than at the top of a page they then have to search. The heading is
focusable and still not a tab stop: it is a heading, not a control.

It also takes an optional place to come back to. F1 and the menu still return focus to
whatever the window is showing; a dialog that opens help on top of *itself* names itself
instead, because the window underneath a modal is inert and focus sent there would land
nowhere.

## Files touched

- `ui/src/controllers/app.ts` — the corrected string and its comment.
- `ui/src/views/main_window.html` — the Help menu and the Help dialog.
- `ui/src/adapters/menu_bar.ts` — `MenuActions.help`, and the new item's id.
- `ui/src/adapters/help_dialog.ts` — new adapter, modelled on `AboutDialog`.
- `ui/src/adapters/keyboard.ts` — F1.
- `ui/src/main.ts` — the wiring.
- `ui/test/adapters/help_dialog.test.ts`, `ui/test/adapters/keyboard.test.ts`,
  `ui/test/adapters/menu_bar.test.ts`, `ui/test/controllers/app.test.ts`.
- `e2e/test/specs/menu.spec.ts` — the bar's order, About's walk, and a Help suite.

## Definition of done

- [x] The announcement is the sentence in decision 1, and the test that pins it says so.
- [x] F1 opens Help from the edit field, from the results buffer, and from the window with no
      session.
- [x] The Help menu opens the same dialog as F1.
- [x] Opening Help twice does not throw, the way About already does not.
- [x] Closing Help returns focus to whatever the window is showing.
- [x] Tab cycles inside Help rather than dropping into the document.
- [x] Opening Help announces one short line rather than reading the whole topic, asserted by
      a test that checks what the description points at rather than that it exists.
- [x] The e2e suite walks the bar's new order, opens Help by F1 and by the menu, and asserts
      the topic's shape against the shipped markup.
- [x] `npm test` in `ui/`, `npm run typecheck`, `npm run test:e2e`, workspace tests,
      `cargo fmt` and clippy clean.

## What the first push got wrong, and why it is recorded here

**The e2e suite was not in this spec's definition of done, and it went red on CI.**
`menu.spec.ts` walks the bar by counting ArrowRights, so a third top-level menu moved About
out from under `openAbout`, and nothing in that suite touched Help at all. The unit suites
caught the same class of change immediately — `menu_bar.test.ts` asserts the leaf count — and
the gap was that the slower suite was never run.

Two things follow, and the second is the one worth keeping. `openAbout` now says in its own
comment that counting keypresses to reach a menu breaks when a menu is added, and the bar's
order is pinned by one test rather than implied by five. And **a change to the menu bar or to
a dialog runs `npm run test:e2e` before it is pushed**, which this definition of done now
says out loud — a spec that lists the fast suites and omits the slow one is a spec that
invites exactly this.

## Accessibility checklist for the PR body

**Agent-observed**, all six, driving NVDA 2026.1.1 through the screen-readers bridge on
2026-08-27: silent capture, `user` persona, the real application. Nothing here needed a sense
the bridge cannot capture, so no item is left for a human — but the finding in decision 5 was
found here and nowhere else, and is worth a listener's own ear.

- [x] The new sentence is announced, in full, and is understandable without knowing how Acter
      works. Heard on the unmarked scripted session: `You will hear what commands print here,
      but not whether they worked. Press F1 for help.`, 4.5 s after the prompt.
      **The item named the wrong session and is corrected.** It asked for this over SSH, where
      it is deliberately never said: the connection sentence has already named the far end and
      said the same thing, so `app.ts` suppresses this one (B9, decision 2). Confirmed by
      waiting the grace period out over SSH and hearing nothing. This is the second checklist
      item written from the backend's event order without checking that suppression — the
      first was B6.2's, the day before.
- [x] F1 opens Help and the dialog announces itself. Heard from the window with no session,
      from the connected session's edit field, and from the results buffer.
- [x] The topic can be read with the arrow keys, line by line, and skimmed with `h`: three
      level 2 headings under the level 1 title, each announced with its level, and the
      paragraphs arrow one line at a time. Decision 4 is doing what it was written for.
      **Opening it read the whole topic aloud at first**, headings omitted — fixed in this PR
      as decision 5, and re-driven: it now says one line and the three sections are still where
      the heading key finds them.
- [x] Tab inside Help does not land outside it. With one control it stays rather than cycling,
      which is `dialog_tab.ts`'s deliberate answer to a single-control dialog and what About
      already does.
- [x] Escape closes Help and focus lands somewhere a listener can carry on from: the Connect
      button in a window with no session, the command line in a connected one.
- [x] Help is reachable from the menu bar — `Help submenu 2 of 3`, then `Acter help` — and
      announces the same dialog, word for word.
