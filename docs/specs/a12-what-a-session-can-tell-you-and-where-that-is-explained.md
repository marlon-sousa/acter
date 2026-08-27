# A12 — What a session can tell you, and where that is explained

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

### 5. The topic explains the difference by what a listener hears, not by what a shell emits

No "OSC 133", no "markers", no "integration" as a thing to understand. The topic answers three
questions in a listener's own terms: what you always get, what you sometimes do not get, and
which sessions are which. Headings so it can be skimmed with `h`.

### 6. What this does not touch

The connection sentence still says "…bash, with no shell integration set up on this host". It is
the same vocabulary and the same objection applies to it, but it was decided and measured in B9
and 27.5 barely a day ago, and rewording it is a change to a *different* sentence with its own
reasons. Filed as roadmap 13.8 rather than done quietly here.

## Files touched

- `ui/src/controllers/app.ts` — the corrected string and its comment.
- `ui/src/views/main_window.html` — the Help menu and the Help dialog.
- `ui/src/adapters/menu_bar.ts` — `MenuActions.help`, and the new item's id.
- `ui/src/adapters/help_dialog.ts` — new adapter, modelled on `AboutDialog`.
- `ui/src/adapters/keyboard.ts` — F1.
- `ui/src/main.ts` — the wiring.
- `ui/test/adapters/help_dialog.test.ts`, `ui/test/adapters/keyboard.test.ts`,
  `ui/test/adapters/menu_bar.test.ts`, `ui/test/controllers/app.test.ts`.

## Definition of done

- [ ] The announcement is the sentence in decision 1, and the test that pins it says so.
- [ ] F1 opens Help from the edit field, from the results buffer, and from the window with no
      session.
- [ ] The Help menu opens the same dialog as F1.
- [ ] Opening Help twice does not throw, the way About already does not.
- [ ] Closing Help returns focus to whatever the window is showing.
- [ ] Tab cycles inside Help rather than dropping into the document.
- [ ] `npm test` in `ui/`, `npm run typecheck`, workspace tests, `cargo fmt` and clippy clean.

## Accessibility checklist for the PR body

- [ ] Connecting over SSH announces the new sentence, in full, and it is understandable without
      knowing how Acter works.
- [ ] F1 opens Help and the dialog announces itself.
- [ ] The topic can be read with the arrow keys, line by line, and skimmed with `h`.
- [ ] Tab inside Help cycles and never lands outside it.
- [ ] Escape closes Help and focus lands somewhere a listener can carry on from.
- [ ] Help is reachable from the menu bar and announces the same dialog.
