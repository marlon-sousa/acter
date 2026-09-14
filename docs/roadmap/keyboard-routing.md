# Keyboard routing and the changed row — history

Entry bodies for this lane. The board is [../ROADMAP.md](../ROADMAP.md); this file holds
what shipped, what was measured, and the findings behind each entry.

Three entries, agreed in conversation 2026-08-31 and recorded in DESIGN.md under "Edit
field ownership", "A row that changed is an answer" and the keystroke map. They are
grouped rather than filed into a lane because they cross both: 28 is frontend routing plus
a domain rule, 29 is domain alone, 30 is a measurement before it is anything. Nothing here
is gated on Convergence, but 28 is only worth running against a real far end, so B4 and B5
in practice.

**This is unfamiliar ground, and the entries say so rather than pretending otherwise.**
Every rule in this section is about what a listener hears when a far end *redraws* a row.
The project has no experience of that: the buffer has always been fed by text arriving,
never by text changing under a key the user just pressed, and every pacing decision so far
has treated a rewrite as churn to be ignored. Expect each of these to open with a session
on the screen-reader bridge or on a real pseudoconsole, and expect at least one of them to
change shape once it meets a real far end. An entry that turns out to have specified the
wrong thing has still done its job if the measurement is written down — that is the same
bargain 22.11 and 23.14 struck, and both paid.

28. **Done** — far-end-line mode: the keyboard goes to the far end, and the row it redraws
    is what you hear. Spec:
    [28-far-end-line-mode.md](../specs/28-far-end-line-mode.md). **23.5 is folded into it** and
    marked Done above. What shipped: `Ctrl+Shift+K` and `set_line_owner`; named `Key`
    variants and `policies::key_bytes`, the measured table, with Backspace pinned as `0x7f`
    because `0x08` deletes the previous *word* in PSReadLine and in `cmd.exe`;
    `TerminalEngine::cursor` and `modes`; `policies::far_end_row`, decision 6's two steps as
    a pure function over the revisions of one settled batch; `LineId` and `LineRevision` on
    the wire, so the buffer applies revisions by id and `Pump::due` stops dropping rewrites;
    Enter's anchored-row rule for headings; and the frontend's ARIA text box, whose shape was
    measured before it was chosen (`ui/src/adapters/far_end_field.ts` carries the run).
    DESIGN's "Edit field ownership" and the open question under it were amended in the same
    PR, since the element decision is a reversal of a Decided one. The reasoning below is
    kept as the record of how the entry got here.

    **Agreed 2026-08-31**, from the user's ask: up and down, Tab, and Ctrl plus any key
    have to be able to reach the far end, and a session inside an `ssh`, a `wsl`, a
    container or a REPL is unusable without them.

    The whole of the reasoning is in DESIGN.md and is not repeated here. What this entry
    builds is: the toggle and its binding (Ctrl+Shift+K) with an announcement that says what
    the user gains and loses rather than naming a mode; an edit field that holds no text
    while the mode is on; routing for every key that is not layer 1; and the cursor-row
    diff that turns the far end's redraw into something spoken.

    **Most of it already exists, which is the reason this is one entry and not a phase.**
    The quiescence clock decides when a row has settled. The positional echo rule from B4.9
    already knows that text appended to the cursor row after Acter wrote to it is the user's
    own typing, and that rule generalises from a line to a character with no change. The grid
    has been carried since B3 precisely so that a question like "what is on the cursor row"
    has an answer. What is new is a comparison and a state.

    **The one piece with a real trap in it is key encoding, and it does not belong in the
    frontend.** An arrow key is not one byte sequence but two: `ESC [ A` normally, and
    `ESC O A` when the far end has turned on application cursor keys (DECCKM), which is what
    readline-driven shells and most TUIs do the moment they take the keyboard. A frontend
    that hard-codes one of them works at a bare `cmd` prompt and sends garbage into `bash`,
    and the failure is silent — the far end simply does something else. The grid already
    tracks that mode because it emulates the terminal, so the mapping from a key to bytes
    belongs beside the grid, in the domain, and the frontend sends a named key rather than a
    byte string. Same question, same answer, for Home, End, delete, the function keys, and
    for whether bracketed paste needs to be honoured when a user pastes into this mode.

    **Measured 2026-08-31, and it is milder than that paragraph claims — the claim is
    corrected rather than left standing.** `bash` inside WSL never sets DECCKM: `ESC [ ? 1 h`
    appears nowhere in the stream, and readline answered `ESC [ A` and `ESC O A`
    *identically*, because it binds both. So neither of the two shells measured can be got
    wrong by choosing one encoding, and the silent-garbage failure this paragraph predicted
    did not occur. What survives is the architectural half, and it survives on weaker
    grounds: a full-screen program is still entitled to set the mode, the grid is still the
    only thing that knows, and a frontend sending a named key rather than a byte string costs
    nothing and cannot be wrong later. Build it that way because it is the right seam, not
    because a measurement demanded it.

    **What the same run did turn up is bracketed paste.** `bash` sets `ESC [ ? 2 0 0 4 h` at
    every prompt and clears it on submission — so pasting into far-end-line mode has to wrap
    the text in `ESC [ 2 0 0 ~` and `ESC [ 2 0 1 ~` or the far end will run each pasted line
    as it arrives, which is a data-loss shape rather than a cosmetic one. The same sequence
    is also evidence, and entry 29 uses it.

    **Experiments to run before the spec is written**, in the order they answer things:

    - Does the field need `role="application"` while the mode is on, and what does a reader
      say when it lands on an edit field that is permanently empty? Both are probeable on
      the bridge with a static page, ahead of any frontend work — the open question in
      DESIGN.md carries the reasoning and the reason the earlier reversal may not apply.
    - At a real `bash` over `ssh`: press up, and check that the recalled line is what gets
      spoken, once, and that the prompt on the same row is not spoken with it. The row
      contains both; what a listener wants is the line, and where the prompt ends is a
      question this project has spent entries on already.
    - Type three characters and press Tab, and check the completion is heard without the
      three characters being read back.
    - Ctrl+C, Ctrl+D and Ctrl+U at a far end that is not the shell Acter spawned, since the
      first two are the reason 23.5 exists and the third is the "what do we say about an
      empty row" string question.
    **Three of those were run on 2026-08-31 and their results are already folded into
    DESIGN.md**, so the spec starts from measured behaviour rather than from this list. In
    summary: NVDA did not switch to focus mode at a plain empty edit field and the arrows
    never reached the page, while the same field inside `role="application"` received up,
    down, Tab and Ctrl combinations — but an `<input>` there makes NVDA say "blank" before
    every single arrow, which is why the element is a real decision and not a detail. At a
    real `bash`, the first up arrow *appends* the recalled line rather than rewriting the
    row, so the speech rule cannot key on revisions; and the row must be spoken from the
    prompt's anchor column, or the prompt is read aloud on every press. Tab's whole
    contribution to the wire was the two bytes `o `, which is worth nothing spoken and is
    the clearest argument for the anchor rule there is.

    **What is still owed before the spec**: the element choice above, Ctrl+D at a far end
    that is not the shell Acter spawned (23.5's subject), and what is said for a row that a
    key emptied.

    **Answered 2026-09-02, and the entry is now specifiable.** A design session with the
    user measured four programs on a real pseudoconsole through Acter's own engine (rig:
    `crates/acter-transports/examples/capture.rs`) and read NVDA's own configuration, and
    the results are written into DESIGN.md rather than repeated here. In summary:

    - **The element question dissolved.** The tester's NVDA has
      `autoPassThroughOnFocusChange` off, so the 2026-08-31 "did not switch to focus mode"
      finding described a configuration, not the reader. With it off no ARIA role
      auto-switches, and the one that forces keys through — `role="application"` — is
      already measured (readable_field.ts) to stop the arrows reading prose. So: no region,
      no key sink, no new element. The edit field stays, NVDA+Space is the handoff.
    - **No renderer either.** The buffer applies revisions by id, blanks included, and a
      `gh` prompt answered with Cancel resolves itself into one line carrying the question
      and the answer — the far end writes the transcript record itself.
    - **The rule is "the row that was rewritten after a key Acter sent"**, not the cursor
      row: `gh` hides the cursor and parks it off the list. Candidate for choosing between
      the two rows that change: speak the one that gained non-whitespace content.
    - **Enter and headings are solved by an existing Decided rule.** The anchored row at
      the instant Enter is sent *is* the far end's echo, so the echo test applies one step
      earlier; a widget answer leaves an empty anchored row and earns no heading. History
      stays out, now as a decision.
    - **One new capability**: cursor position on `TerminalEngine`, for left and right
      arrows, which rewrite nothing and are therefore invisible to every diff rule.

    Still owed: the "blank" probe on an empty `<input>` whose arrows are prevented, Ctrl+D
    at a far end (23.5's subject), and the emptied-row string.

    **Deliberately not in this entry**: any renderer, the alternate screen, and the
    several-rows case, which is 30.

28.1. **Done** — Acter wrote the row after the reader had stopped waiting for it. Spec:
    [28-far-end-line-mode.md](../specs/28-far-end-line-mode.md), amendment F. Fixed in 28's own
    PR. It reversed no decision of 28.
    **Found 2026-09-02 in 28's own NVDA pass**, driving a real WSL `bash`
    through the screen-readers bridge as the `user` persona, NVDA 2026.1.1, silent capture.

    **What a listener meets.** In far-end-line mode, every key is answered with the state
    *before* it. Two commands in history, then up arrow: NVDA said "em branco" — blank —
    because the field was still empty at the instant of the press. Up arrow again: NVDA said
    `echo acter-history-two`, the line the *previous* press had recalled, while the field by
    then held `echo acter-history-one`. Home said "blank". Right arrow from the start of the
    line said "e", "c", "c" — one behind, and repeating. Tab and Backspace said nothing at
    all.

    **The row and the caret Acter writes are correct**, which is what makes this a timing
    defect rather than a content one: `nvda+uparrow` re-read the field on demand and got the
    right line every time, and Tab's completion (`ech` to `echo `) and Backspace's single
    character deletion were both exactly right in the field. What is wrong is only *when*.

    **The first diagnosis written here was wrong, and the correction is the whole entry.**
    It said NVDA answers a caret command synchronously from the field as it stands, and that
    the defect therefore could not be fixed by writing sooner. **NVDA does not answer
    synchronously. It waits.** Read in NVDA's own source, `source/editableText.py`:
    `EditableText._caretMovementScriptHelper` takes a bookmark of the caret, sends the key on
    with `gesture.send()`, and then calls `_hasCaretMoved`, which polls the caret every 10 ms
    until it moves or until `config.conf["editableText"]["caretMoveTimeoutMs"]` elapses —
    **default 100 ms, and exposed to the user in Advanced settings as "Caret movement timeout
    (in ms)", 0 to 2000**. Only then does it speak what is at the caret. The arrow keys are
    bound to those scripts (`__gestures`, `caret_moveByCharacter` and `caret_moveByLine`), and
    our field gets that behaviour because `IAccessible.findOverlayClasses` attaches
    `EditableTextWithAutoSelectDetection` to any object with an `IAccessibleTextObject` whose
    role is text or whose state is editable — which is what a `contenteditable` in WebView2 is.

    **So the model Acter needs is the one NVDA already assumes for a terminal**, and it is not
    an invention: `NVDAObjects.behaviors.Terminal` is `LiveText` plus `EditableText`, and
    `WinConsoleUIA._caretMovementTimeoutMultiplier` raises the poll to 1.5x with the comment
    "On older consoles, the caret can take a while to move." A terminal is expected to repaint
    late; the reader's job is to wait for the repaint and then speak it. Acter is simply
    later than the window it is allowed.

    **How much later is measured, and the margin is enormous.** New rig,
    `crates/acter-transports/examples/latency.rs`, which sends a key to a real pseudoconsole
    and timestamps every line and cursor change the engine reports, in milliseconds from the
    byte going out. Measured 2026-09-02 against `bash` under WSL, Windows PowerShell and
    `cmd.exe`, on a warmed prompt with history:

    - `bash` under WSL: left 1 ms, Home 0 ms, up arrow 3 ms, Backspace 4 ms.
    - Windows PowerShell: all four keys 0 ms.
    - `cmd.exe`: left 0 ms, Home 0 ms, up 0 ms, Backspace 1 ms.

    In every case the answer arrived **in a single batch — first change and settled are the
    same instant**. The far end answers in single-digit milliseconds against a 100 ms window.
    **Acter's 500 ms quiescence clock is the entire defect**: it is five times the window the
    reader gives, and one hundred times what the far end takes. (The first run of the rig
    reported 226 and 351 ms and was thrown away — that was WSL still starting, not a keypress.)

    **A second defect the rig found, independent of the clock.** Left, right, Home and End
    **rewrite no line at all** — the far end only repositions the cursor, and the engine
    reports no `TerminalItem::Line` for any of them. The cursor is the whole of the evidence
    that the key arrived. `policies::far_end_row`'s third step exists for exactly this and is
    correct, so the pump does answer them; but it means any future change that keys the
    far-end path off changed lines alone would silently lose the four commonest navigation
    keys.

    **What this makes the fix.** Not a live region, not `role="application"`, and no reversal
    of decision 2 or 3 — the element is right and the content it holds was right all along.
    The far-end line comes off the pacing clock: `Pump::run` arms its settle timer with
    `self.quiescence`, and that number belongs to the transcript, not to a keystroke a reader
    is standing there waiting for. It needs its own, measured, much smaller number, chosen to
    coalesce a multi-write redraw while landing well inside 100 ms. Everything downstream is
    already fast enough: `SessionActor` forwards `SessionInput::FarEndLine` straight to the
    sink with no render tick, and `far_end_field.render` writes the text and the caret in one
    DOM update, which is one bookmark change for the poll to catch.

    **And there is an honest answer for a far end that is genuinely slow**, which is the same
    one NVDA gives for old consoles: the timeout is the user's, up to 2000 ms. That is a line
    of documentation, not code.

    **The fix** is `PacingConfig::far_end_settle`, default 30 ms, used for a settling a key is
    outstanding for; everything else keeps `quiescence`. Pinned by
    `a_keystroke_is_answered_while_the_reader_is_still_listening` and
    `a_redraw_that_arrives_in_pieces_is_coalesced_into_one_answer`, both of which fail against
    the code as it was, and by
    `output_nobody_pressed_a_key_for_keeps_the_pacing_clock`, which pins what must not change.

    **Re-checked on the reader 2026-09-02**, NVDA 2026.1.1, silent capture, `user` persona, at
    a real `bash` under WSL Ubuntu. Every item this blocked now passes. Up arrow said
    `echo acter-history-two` then `echo acter-history-one`, one recall per press, where before
    it said "em branco" and then the previous press's line. Home said "e" and three rights said
    "c", "h", "o", where before it said "em branco" and then "e", "c", "c". Backspace spoke the
    character it deleted and left the row exactly one shorter, where before it said nothing.
    The selection prompt speaks one option per press. **Tab is the one key that is still
    silent, and it is not this** — see 28.4.

    Blocks the four checklist items that failed in 28's PR body — up arrow, Tab completion,
    left and right, and Backspace's spoken half — and it is what keeps 28.2's item unchecked
    now that 28.2 itself is fixed: the `gh` selection reaches the field one option per press
    and is still not spoken on the press.

    **The smaller thing recorded here turned out to be its own defect** and is now 28.5. What
    follows is what was seen at the time, because it may turn out to be the same clock: after `Ctrl+C` killed `gh`, the
    field briefly held the whole prompt row — `marlon@splyt:/mnt/c/Users/marlo$` — because
    the anchor was taken at a settling that landed part-way through the far end drawing its
    prompt, so the anchor column was zero. No spurious heading came of it in that session,
    and it was not chased further.

28.2. **Done** — the content rule never ran, so a `gh` prompt said nothing. Spec:
    [28-far-end-line-mode.md](../specs/28-far-end-line-mode.md), decision 6. Fixed in 28's own
    PR, in the commit after the pass that found it. **Found 2026-09-02**, on a real
    `gh repo create` aborted at its first prompt.

    **What a listener meets.** Arrowing the selection produces silence. The field stays
    empty and stays empty.

    **The far end and the engine both did their part**, which is what makes this a plain
    bug: the buffer afterwards holds the whole interaction as four rows — the question, the
    three options with `>` on the one the arrow moved to, and the returning prompt — so the
    row gained its marker, the engine reported it, and `policies::far_end_row` would have
    picked it. It was never asked.

    **Where.** `Pump::far_end_settled` branches on `self.far_end.anchor.is_none()` to
    re-anchor instead of asking the policy. That condition was written for one case — the
    settling right after Enter, where the anchor is deliberately cleared — and it also
    catches the case the content rule exists for: a far end that hides its cursor has no
    anchor at all, by 28's own amendment B, so step 2 is unreachable exactly where step 2 is
    the answer. The two states need telling apart rather than sharing `None`.

    **The fix** is an explicit `awaiting_prompt` flag on the pump's far-end state, so the two
    meanings of "no anchor" stop sharing an `Option`: a submission sets it and the settling
    after that consumes it, and everything else with no anchor goes to the policy, whose
    second step is the answer. Pinned by
    `a_widget_that_hides_its_cursor_still_gets_the_content_rule`, which reproduces the
    sequence that found it — the widget takes the screen while Acter still owns the line, so
    no anchor is ever taken — and fails against the code as it was.

    **Re-checked on the reader 2026-09-02**, NVDA 2026.1.1, silent capture, `user` persona,
    at a real `gh repo create`: the field now holds `> Create a new repository on github.com
    from a template repository` after one press and `> Push an existing local repository to
    github.com` after the next, so the selection reaches the listener one option per press.
    **It is still not *spoken* on the press** — 28.1 stands in front of it, and that item's
    checklist box stays unchecked until 28.1 is answered.

28.3. **Done** — F6 could not reach the results buffer while the far end owned the line.
    Spec: [28-far-end-line-mode.md](../specs/28-far-end-line-mode.md), decision 2. Fixed in 28's
    own PR, in the commit after the pass that found it. **Found 2026-09-02.**

    **What a listener meets.** F6 in far-end-line mode does nothing: NVDA re-read the
    far-end field and focus never moved. Review by heading is unreachable, which is the whole
    of how this product is meant to be read back — and it is the same complaint the
    2026-09-02 amendment under "Edit field ownership" made about headings, reached from the
    other side.

    **Where.** `AppController.toggleFocusArea` knows two areas and asks
    `editField.isFocused()`; in this mode the edit field is hidden and unfocused, so the
    toggle focuses a hidden `<input>`, which does nothing at all. It has to ask which line is
    in front of the user and toggle between *that* field and the buffer.

    **The fix** is one question asked of the state rather than of an element:
    `AppController` now resolves *which line is in front of the user* and toggles between
    that and the buffer, and `Escape` from the buffer returns to the same one. Escape *at*
    the far end's line stays the far end's, and the keyboard adapter tells the two apart by
    `defaultPrevented` — the field consumed it — rather than by asking which mode is on,
    which is one less thing to keep in step.

    **Re-checked on the reader 2026-09-02**: F6 from the far-end line landed on "Results
    região, gh repo create, título nível 2", and F6 again came back to "Command line" rather
    than to the hidden local field. Both directions, both announced.

28.4. **Done** — Tab completion was applied silently. Spec:
    [28-far-end-line-mode.md](../specs/28-far-end-line-mode.md), amendment I. Fixed in 28's own
    PR, after two rounds of element measurement and one of reading NVDA's source. **Found 2026-09-02**, re-running 28's checklist on NVDA 2026.1.1 after 28.1 was
    fixed, silent capture, `user` persona, at a real `bash` under WSL.

    **What a listener meets.** `ech` then Tab: no speech at all. The completion is correct —
    the field held `echo ` and `nvda+uparrow` read it back — so nothing is wrong with what
    Acter wrote or when it wrote it. Every caret key around it now speaks on the press.

    **The cause.** NVDA's poll-then-speak behaviour, which is what 28.1's fix aims at, lives
    in `EditableText`'s caret-movement scripts, and its `__gestures` table binds the arrows,
    `home`, `end`, the page keys, Enter and Backspace. **Tab is not bound there.** In focus
    mode Tab means "announce the newly focused object"; the field prevents it, focus does not
    move, and NVDA has nothing to say. No clock reaches this, because no clock is running.

    **A real terminal gets it from the other half of the same object.**
    `NVDAObjects.behaviors.Terminal` is `LiveText` *plus* `EditableText`. The caret scripts
    answer the arrows; `LiveText` monitors the object's text and speaks what changed, and that
    is what carries Tab completion in a console. Acter's field is an ARIA text box: it has the
    first half and not the second.

    **The element was re-measured, and the element is not the answer.** Probe
    `ui/probes/element_probe.html`, run 2026-09-02 on NVDA 2026.1.1 through the bridge, silent
    capture, `user` persona, in Edge — WebView2's own engine. Eight variants, each behaving
    exactly as the far-end field does (every key prevented, text and caret written by script),
    differing only in role, ARIA and how the completion is applied. `ech` then Tab in each:

    - **A, `role="textbox"` as shipped: silent.** The probe reproduces the defect, so it is
      faithful.
    - **B, textbox plus `aria-autocomplete="inline"`: silent.** NVDA does see the attribute —
      it announces "possui autocompletar" when focus lands — and still says nothing when the
      text changes.
    - **C, `role="combobox"` plus `aria-autocomplete="inline"`: silent**, and it makes the
      field announce itself as "caixa de combinação recolhido multilinha editável abre lista"
      — a collapsed combo box promising a list that does not exist. Worse on arrival and no
      better on Tab.
    - **D, `role="searchbox"`: silent.**

    So **no role announces a programmatic content change**, and decision 2's element stands.
    What does work is a mechanism layered on top of it, and two of them do:

    - **E and H, the inline-autocomplete pattern**: leave what the completion added
      *selected*, and NVDA announces it — **"o  selecionado"** — out of
      `EditableText.detectPossibleSelectionChange`. H collapses the caret 120 ms later, so
      nothing stale is left standing in a field whose contents are the far end's rather than
      provisional, and the announcement still happens. No live region at all. It says what was
      *added*, not the completed line, and it needs the change to be a pure append — a Tab
      with several candidates rewrites the row and has no delta to select.
    - **F, a live region fed only by the completion**: says **"echo"**, the whole row. Caret
      keys are unaffected — Home said "e", right arrow "c", one utterance each, no
      double-speak.

    **And G measured why the always-on version is wrong**, which is decision 3 confirmed
    rather than assumed: a live region fed on *every* answer says everything twice. Typing
    `ech` gave "e", "c", **"ec"**; Backspace gave **"h"** from the reader and then **"ec"**
    from the region. That is the noise decision 3 deleted, reproduced on demand.

    **A second round found the callback, and it is not `aria-autocomplete`.** The question
    asked was whether ARIA has a proper contract for this rather than a workaround, and it
    does. In NVDA's source, `NVDAObject.event_selection` reads: *"This object has been
    selected. If this object's container / parent is being controlled by the focus, then
    report this selection."* It takes `api.getFocusObject().controllerFor` — which
    `NVDAObjects.IAccessible._get_controllerFor` resolves from the IA2 `CONTROLLER_FOR`
    relation, which is **`aria-controls`** — and if the newly selected object is a descendant
    of something the focused field controls, it cancels speech and calls `reportFocus()` on
    it. So the announcing mechanism is *a listbox the field controls, whose selection moves*.
    `aria-autocomplete` is only a state, announced once when focus arrives ("possui
    autocompletar") and never again; and `InputFieldWithSuggestions.event_controllerForChange`
    says only that suggestions *appeared*, in braille and a sound — nothing the bridge can
    hear and no text either way.

    The web agrees, which is worth recording because it means this is not a local quirk.
    a11ysupport.io's `aria-autocomplete` test data has NVDA's support for `inline`, `list` and
    `both` as only partial in Chrome, Edge and Firefox, and **no screen reader at all fully
    conveying `inline`**. And Adobe's React Spectrum team, building a combobox, hit the
    failure J reproduces below: *"character deletions and text cursor movement in the ComboBox
    input weren't being announced at all"*, resolved only by *"clearing option focus on any
    changes to the input text or left/right arrow key presses"*.

    **Three more variants, measured the same way:**

    - **I, the textbox exactly as decision 2 chose it plus `aria-controls` pointing at an
      offscreen listbox whose selection moves: speaks the completion.** "echo 2 de 2". And
      **every caret key still works** — Home "e", right arrow "c", End "em branco", Backspace
      "espaço", one utterance each, no double-speak, because the listbox selection only
      changes on a completion.
    - **K, the same with one option replaced each time: "echo 1 de 1".** `aria-setsize="-1"`
      did **not** suppress the position info, so the count comes along with `reportFocus()`
      whatever is done to it. Caret keys unaffected.
    - **J, the full ARIA 1.2 combobox — expanded, `aria-controls`, `aria-activedescendant`:
      announces the completion and then silences everything else.** Home, right arrow and
      Backspace all produced **no speech at all** while an option held virtual focus. That is
      28.1 undone, measured here and corroborated by Adobe above. **J must never be built.**

    **The recommendation is now I/K rather than F**, and the decision is still the user's.
    I/K is the platform's own contract rather than a workaround; it keeps decision 2's element
    untouched; it needs no table of "which keys this reader speaks for", which was the one
    real weakness in F, since the listbox is fed by a completion rather than by a key; and it
    extends without redesign if Acter ever surfaces several candidates, where "1 of 5" stops
    being noise and starts being the point. **Its cost is the position info** — "echo 1 de 1"
    where F says "echo" — which could not be removed from the page side.

    **What was built is H**, and the question about Tab-Tab below is why. I/K would have
    spoken the whole row on a *completing* Tab, which is more than the press changed; H speaks
    what the completion added, which is what the user asked for and what a listener needs to
    know. It also adds nothing at all to the accessibility tree, where I/K adds a listbox and
    F a live region. The listing Tab, which is the case a whole-row announcement would have
    been for, turned out not to be the field's problem at all — it is 28.6's.

    **The fix**: what the completion added is left *selected*, which NVDA announces out of
    `EditableText.detectPossibleSelectionChange`, and the selection is dropped 120 ms later so
    nothing stale stands in a field holding the far end's line. Only a pure append qualifies,
    and only the answer to a completion key is marked — every other key the reader already
    speaks for, and a selection on top of that would be the double-speaking decision 3
    deleted.

    **Re-checked on the reader 2026-09-02**, NVDA 2026.1.1, silent capture, `user` persona, at
    a real `bash` under WSL: `ech` then Tab said **"o selecionado"**, and
    `ls /tmp/acterprobe/al` then Tab said **"pha- selecionado"**. The field then read
    `ls /tmp/acterprobe/alpha-` on demand and a left arrow answered "hífen", so the caret keys
    are untouched.

    **G and J stay ruled out by measurement**, and their entries above say why.

    **What Tab-Tab does, which is the question that must be answered before choosing.** Asked
    2026-09-02 and measured rather than reasoned about, at a real `bash` under WSL with three
    files sharing a prefix (`/tmp/acterprobe/alpha-{one,two,three}.txt`), typing
    `ls /tmp/acterprobe/al`:

    - **Tab 1** appends the common prefix: the row goes from `...al` to `...alpha-`, a pure
      append of `pha-`, and the cursor moves along the same row. bash also rings the bell.
    - **Tab 2 does nothing at all.** bash sends one `` and no other byte. readline lists on
      a *repeated* completion, and Tab 1 changed the line, so Tab 2 is a fresh attempt.
    - **Tab 3 lists**, and it does so as **two new rows**: `alpha-one.txt    alpha-three.txt
      alpha-two.txt`, and below it the prompt and command line redrawn — and **the cursor
      moves from row 0 to row 2**, to the same column, onto the redrawn command line.

    So **the candidate list is output, not the command line**, and no mechanism in this entry
    would speak it: H selects a delta on the command line and there is none; I/K feed a
    listbox from the command line; F feeds a live region from the command line. The answer to
    "will Tab-Tab say everything it shows" is **no, and not because of the mechanism**.

28.6. **Done** — after a listing Tab the far-end field held the candidate list instead of the
    line being edited, and stayed wrong. Spec:
    [28-far-end-line-mode.md](../specs/28-far-end-line-mode.md), amendment J. Fixed in 28's own
    PR. It was a hole in decision 6, found by asking what Tab-Tab does. **Found 2026-09-02** on NVDA 2026.1.1, silent capture, `user`
    persona, at a real `bash` under WSL.

    **What a listener meets.** After the third Tab, NVDA said the bare prompt row
    (`marlon@splyt:/mnt/c/Users/marlo$`). The field then held
    `alpha-one.txt    alpha-three.txt  alpha-two.txt` — the candidates — and it **did not
    recover**: a left arrow spoke `h`, a character out of the candidate row, and
    `nvda+uparrow` read the candidate list back. The user is editing
    `ls /tmp/acterprobe/alpha-` and everything they hear comes from a row they are not on.
    The candidate list never reached the buffer either, so it is both in the wrong place and
    missing from the transcript.

    **The cause.** The far end drew the list *and* redrew the command line on a new row, so
    two rows gained content. `policies::far_end_row`'s second step answers with the row that
    gained content and picked the list. Its third step, which would have caught this, tests
    for a cursor that moved *along the same row*, and this cursor changed rows — measured,
    row 0 column 58 to row 2 column 58.

    **The shape of the fix, not yet a spec.** The cursor is the evidence and it is
    unambiguous: **when the cursor changes row, the command line has moved, and the row it
    moved to is the command line.** Follow it, re-anchor there, and answer from the new
    anchor — which yields `ls /tmp/acterprobe/alpha-`, the row the user is actually editing.
    The other rows that gained content are then what they look like: the far end printing
    output at its own prompt, which belongs in the buffer and is spoken by the path that
    speaks output. That is also what makes Tab-Tab audible, and it is why **28.6 comes before
    28.4** — the listing case is answered by the transcript, and only the completing case
    needs a mechanism at all.

    **The fix is in two halves.** The anchor follows the cursor to the row the command line
    was redrawn on, and where it begins on that row is measured off by what the listener
    already had, because a `readline` redraw carries no marker to strip by. And the rows that
    changed which are *not* the command line's row are content the far end showed: they now go
    where `Pump::publish` already puts text no submission accounts for, instead of being
    dropped by a region filter that wants only `Output`. Pinned by
    `a_command_line_redrawn_on_another_row_is_followed_there` and
    `what_the_far_end_printed_at_its_prompt_reaches_the_transcript`, both of which fail against
    the code as it was.

    **Re-checked on the reader 2026-09-02** at a real `bash` under WSL with three files
    sharing a prefix: the listing Tab spoke
    **`alpha-one.txt alpha-three.txt alpha-two.txt`**, put that row in the transcript, and left
    the field holding `ls /tmp/acterprobe/al` — the line being edited. Before the fix the field
    held the candidates, a left arrow read a character out of them, and the transcript had
    nothing.

    **One wart left standing, and it is not new**: the bare prompt row the far end redraws is
    published and spoken alongside the candidates, so the listener hears
    `marlon@splyt:/mnt/c/Users/marlo$` first. It behaved identically before this fix, so it is
    not a regression, and it is recorded here rather than chased.

28.5. **Done** — an anchor taken from a prompt still being drawn headed the next block with
    the whole row. Spec: [28-far-end-line-mode.md](../specs/28-far-end-line-mode.md),
    amendment F. Fixed in 28's own PR, in the same commit as 28.1, which introduced it.
    **Found 2026-09-02** while re-running 28's checklist on the reader.

    It was seen once before, during 28's first pass, and recorded under 28.1 as a smaller
    thing not worth chasing. Making the keystroke clock short turned it from rare into
    reproducible, and the re-check caught it doing real damage.

    **What a listener meets.** Answering an inline selection prompt left the transcript with a
    second heading for the same command, reading
    `marlon@splyt:/mnt/c/Users/marlo$ python3 /tmp/acter_menu.py` — the shell prompt and the
    command together, where the command alone belongs, and where decision 7 says no heading
    belongs at all, because answering a prompt is not running a command.

    **The cause.** Enter leaves a key outstanding like any other, so it took the keystroke
    clock. But its answer is not a caret anybody is polling for: it is the far end running a
    command and drawing its next prompt, and the settling after it is where the anchor is
    taken. Thirty milliseconds lands inside that drawing — the prompt is on the row, the
    cursor has not reached the end of it — so the anchor is taken at column zero. Nothing is
    heard at the time. It goes wrong at the *next* submission, which reads the row from
    column zero and heads its block with all of it.

    **The fix** is that the short clock is for `watching && !awaiting_prompt`: a key whose
    answer someone is waiting to hear, and not a submission whose answer is a new prompt.
    Pinned by `an_anchor_is_never_taken_from_a_prompt_still_being_drawn`, which reproduces the
    sequence — submit, the far end echoes onto a new row with its cursor still at column
    zero, then a program hides the cursor so nothing ever re-anchors — and fails against the
    code as it was with exactly the observed heading.

    **Re-checked on the reader 2026-09-02**: the transcript of the same interaction is now one
    heading (`python3 /tmp/acter_menu.py`), the four rows of the prompt with the selection
    where it was left, the far end's own one-line record (`chose: Push an existing local
    repository`) and the returning prompt. No second heading.

28.7. **Done** — you could not tell who had your keys, and the default was backwards. Spec:
    none — the decisions are in DESIGN's "Edit field ownership", amended in the PR that made
    them. Merged as PR #57 (2026-09-02). **Raised 2026-09-02 by the user**, after entry 28
    merged and they drove it themselves.

    **What a listener meets.** Handing the keys over moves focus to a different field, and
    the reader announces the sentence rather than the field it landed on. Measured on NVDA
    2026.1.1, silent capture: turning it **off** announced "Command input edição em branco" —
    you hear where you landed — while turning it **on** announced nothing but the sentence.
    `get_focus_info` showed focus really was on the far-end field, so the field is fine and
    the announcement is what is missing. A listener can therefore be on either line with
    nothing said about which, and the two lines answer the same keys quite differently: on
    Acter's line, up arrow is Acter's history and `Tab` moves focus out of the window; on the
    program's, they are the program's.

    That is not hypothetical. It cost the user and this agent several exchanges arguing about
    which line they had been on, and the agent's first theory was wrong twice over — first
    blaming local-line mode, then finding that NVDA's own browse mode produces the identical
    pair of symptoms, since there `up arrow` reads a document line and `Tab` moves focus.

    **What was decided, in conversation, and what it changes.**

    - **A session starts with the program holding the keys.** Nearly every far end has a line
      editor, and its history, completion and bindings are what a terminal user reaches for;
      starting on Acter's line began every session by taking those away. Acter's line is the
      retreat now — a slow link, or a program that shows nothing while you type.
    - **After every connection, one sentence says which it is and which key changes that**:
      *"Remote process keys. Ctrl+Shift+K changes that."* It follows the connection sentence
      rather than replacing it, and it is said **once** — not when focus lands on a command
      line, which would repeat the same fact after every F6, Escape and dialog.
    - **The vocabulary drops "far end" for the end user**, and the user chose it: the two
      states are **"Acter process keys"** and **"remote process keys"**. `LineOwner::FarEnd`
      stays the domain's word, because it is exactly right in code and means nothing to
      somebody who just wants to run a command.
    - **Two untrue words came out.** The toggle used to answer "Acter gets your keys again.
      History and completion are back." **Acter has no history and no completion** — searched
      2026-09-02, every match in the codebase is the far end's own recall or the string
      itself — so it handed back a feature that never existed. Reported by the user, removed
      rather than reworded.
    - **The help dialog gains a section**, "Who gets your keys", carrying the trade — whose
      history, whose completion, and when Acter's line is worth taking back. One sentence is
      what you hear; the dialog is what you can re-read.

28.8. **Done** — who gets your keys is remembered per connection. Spec:
    [26-connection-manager.md](../specs/26-connection-manager.md), decision 11. **Raised
    2026-09-02 by the user in the same conversation as 28.7**, parked by them with "this
    will come later", and closed by entry 26 without a setting of its own.

    **What was missing was the saved half.** The state is per session by design — a mode
    carried across connections would change what a key does in a shell the user never chose
    it for — and there was nowhere to keep the answer a connection had been given last time.
    There is now: a saved connection records who owns the line at the moment of saving,
    alongside whether Acter may set the session up, and `Connected` carries that back so the
    window applies it where it already decides which owner a new session starts on. A saved
    choice wins over the default there; a connection nobody has saved still opens on the far
    end's line.

    **No new setting, and that is the point.** Saving writes the session as it stands, so a
    connection that always wants Acter's line is made by taking the line and then saving —
    which is the same gesture as keeping a changed port.

28.9. **Done** — a trailing space was invisible, so deleting one was silent. Spec:
    [28-far-end-line-mode.md](../specs/28-far-end-line-mode.md), amendment K. **Found 2026-09-02
    by the user**, driving 28.7 with the remote process holding the keys.

    **What a listener met.** Backspace over a space said nothing at all. Every other
    character is announced as it is deleted.

    **The cause, measured** with `acter-transports/examples/capture.rs` at a real `bash`:
    typing `echo hi` then a space produced **no line item**, the backspace that removed the
    space produced **no line item**, and only the next backspace — which took the `i` —
    produced `Rewritten "...echo h"`. Acter's row text is trimmed, so `echo hi ` and
    `echo hi` are the same string to it: the field never held the space, nothing changed when
    it went, and the reader correctly said nothing about a field that did not change.

    **The trimming is right and stays.** `acter-term`'s extractor trims because a grid row
    is padded with spaces to its full width, and an untrimmed walk "speaks eighty spaces after
    every line" (decision 9). What was wrong is only that the far-end field then rendered a
    line shorter than the far end's own cursor.

    **The fix.** The cursor is the evidence again: **a caret beyond the end of the row means
    there is whitespace there**, so the row handed to the field is padded out to the caret.
    `echo hi ` now reaches the field with its space, deleting it is a change the reader
    announces, and a caret can never sit past the text it is given. It is in
    `policies::far_end_row`, where the caret and the text are decided together, and not in the
    extractor — which needed one more input there: the text the field is holding, whose
    trailing spaces are the padding the policy added last time. Pinned by
    `a_space_typed_at_the_end_reaches_the_field_as_a_space`,
    `deleting_a_trailing_space_shortens_the_line_the_listener_holds`,
    `a_caret_past_the_text_pads_the_row_out_to_it` and the service test
    `a_trailing_space_is_in_the_field_and_deleting_it_is_a_change`.

    **One thing the entry did not foresee, and the tests forced.** Backspace over a trailing
    space and a left arrow across one are the *same* grid state — the row reads `echo hi`
    either way and only the cursor moved, one column left — so nothing can tell them apart and
    the padding is re-measured for both. What can be told apart is a cursor that came to rest
    *inside* the row: it says nothing whatever about the whitespace after it, so the line the
    listener holds is left alone and `Home` stays a caret move rather than becoming a row
    reread (`a_caret_moving_inside_a_padded_line_still_rewrites_nothing`).

    **Re-checked on the reader 2026-09-02**, NVDA 2026.1.1, `user` persona, live capture, at
    an integrated WSL Ubuntu with the remote process holding the keys. Typing `echo hi` and
    then a space, the backspace that removes the space says **"espaço"**, and the next
    backspace says **"i"** — the space is announced exactly as every other character is,
    where before it said nothing at all.

28.10. **Done** — in an integrated session the prompt was announced on every completion
    redraw. Spec: [28-far-end-line-mode.md](../specs/28-far-end-line-mode.md), amendment L, and
    [b5.6-the-prompt-is-spoken.md](../specs/b5.6-the-prompt-is-spoken.md), amendment A. **Found
    2026-09-02 by the user**, at an integrated Ubuntu with the remote process holding the
    keys. Typing `cd a`: the first Tab said nothing, the second repeated the prompt, and the
    list of matches was never read.

    **Why it is integrated-only, and it is not Acter's doing.** `readline` re-emits the whole
    prompt string on every redraw, invisible parts included — and an integrated session's
    `PS1` carries the OSC 133 markers, because that is where Acter's setup puts them.
    Captured with the markers in `PS1`: **`marker PromptStart` appears on every Tab.**
    `Pump::drawn` then treated each redraw as a prompt being drawn and announced it. An
    unintegrated `PS1` carries no markers, no `PromptStart` is emitted, and nothing is
    announced — which is why the user found this worked unintegrated and not integrated.

    **The sequence, measured** at the same shell, `cd a` in a directory with two matches: Tab
    one sends a single bell and nothing else, and **Tab two** both lists and redraws. (It
    differs from the `ls` case recorded under 28.6, where Tab one appended a common prefix and
    so pushed the listing to Tab three: readline lists on a *repeated* attempt, and whether an
    attempt changed the line is what decides which press that is.)

    **The fix, and it is an amendment rather than a quiet condition.** The rule it touches is
    spec B5.6 decision 3 — "every prompt, not only the ones that changed" — and not B4.5
    decision 4, which is the separate arm that makes a cmd prompt block *content*; the entry
    named the wrong one. Amended: **a prompt drawn with no command between it and the last is
    that prompt being repainted, and is not announced again.** A command starting or ending is
    what makes the next prompt news. The two are told apart by what happened in between and
    never by the text, because an ending prompt is usually identical to the one before it —
    which is why "announce it when it differs" was rejected in B5.6 and stays rejected. Pinned
    from both sides: `the_same_prompt_after_a_command_is_still_announced` and
    `a_completion_redraw_does_not_announce_the_prompt_again`.

    **The unread candidate list was a hypothesis, and the service test disproved it.** The
    theory was that the marker traffic changed which region the list row was labelled with, so
    an integrated session's filter turned it away. Built from the batch `capture.rs` recorded
    at a real integrated `bash` — `line "alpha/ axel/"`, `marker PromptStart`, the redrawn
    prompt row, `marker CommandStart`, the command line — it is **wrong**: the list arrives
    *before* the redraw's `PromptStart`, so the tracker is still where the last `B` left it,
    and 28.6's publishing path takes it from there. It reaches the transcript **and** it is
    read aloud, in the very batch the user described
    (`the_candidates_reach_the_transcript_and_are_read_aloud`). Nothing in the domain was
    dropping it. What stood in front of it was the prompt announcement, arriving first, over a
    listener editing a line — so removing that is the whole of the fix.

    **Re-checked on the reader 2026-09-02**, NVDA 2026.1.1, `user` persona, live capture, at
    an integrated WSL Ubuntu — integration confirmed on the spot by `false`, which announced
    "command failed, exit code 1". In `~/p28` holding `alpha/` and `axel/`, typing `cd a`:
    the first Tab said nothing, and **the second read "alpha/ axel/" with no prompt before
    it**. So the list is heard, which is what the service test predicted: it was always on
    the read-aloud path and the prompt announcement was what stood in front of it. The field
    still held `cd a` afterwards, and a history recall spoke the recalled line and no prompt.
    The other side holds too: `cd alpha` announced the new prompt, and `true` in that same
    directory announced the identical prompt again.

28.11. **Done** — a failing command was announced again at every empty Enter. Spec:
    [b6-session-service.md](../specs/b6-session-service.md), decision 8, amendment A — it adds a
    fourth `SessionInput`, so it is proposed in the amendment rather than made quietly.
    **Found 2026-09-02 by the user**, at an integrated Ubuntu: `gh pr create`, `Ctrl+C`, and
    then "command failed, exit code 2" on every press of Enter, however many times they
    pressed it.

    **What a listener meets.** A command fails. The verdict is announced, correctly. Then
    every subsequent Enter on an **empty** command line announces that same verdict again,
    with no command having run in between — so a listener who presses Enter to feel where
    they are is told three times that something failed, and nothing distinguishes the third
    telling from a fresh failure.

    **Reproduced on the reader 2026-09-02**, NVDA 2026.1.1, `user` persona, live capture, at
    an integrated WSL Ubuntu with the setup accepted, and it is **smaller than the recipe it
    was found with**. Neither `gh` nor `Ctrl+C` is needed:

    - `false` and Enter announced "command failed, exit code 1". Three empty Enters after it
      announced **"command failed, exit code 1"** each time.
    - The user's own sequence, with `sleep 100` standing in for `gh` because the throwaway
      worktree `gh` needed was unusable from WSL: `Ctrl+C` said "^C", then "command failed,
      exit code 130", and every empty Enter after it repeated **exit code 130**.
    - **A successful command clears it.** After `true`, an empty Enter announces the prompt
      and no verdict.
    - **It is not far-end-line mode.** `Ctrl+Shift+K` back to Acter process keys and the same
      three presses behave identically.

    **The cause, and Acter is being told the truth.** `__acter_prompt` prints `D;$?` before
    every prompt. An empty command line runs nothing, so bash leaves `$?` at the last real
    command's code and honestly re-reports the same failing `D` at the next prompt. Acter
    believes each `D` and announces it again. That accounts for both boundaries exactly: only
    an integrated session has a `D` at all, which is why an unintegrated one is silent here,
    and a successful command resets `$?`, which is why `true` ends it.

    **What the service test established, and it killed both first guesses.** Built from the
    same capture and run against the code as it stands:

    - **An empty Enter is a whole `C..D` cycle.** The measurement is plain: `OutputStart`,
      the prompt row settling, `CommandEnd(1)`, `PromptStart`, `CommandStart` — a block
      really does start, because `PROMPT_COMMAND` itself trips the `DEBUG` trap and prints
      `C`. So "a verdict with no command started since the last one" — the symmetry with
      28.10, and the tempting rule — is **false here** and would have been written for a
      mechanism that does not exist.
    - **Nor does the empty submission own the block.** `submit("")` took `CommandId(2)`, and
      what opened was **`CommandId(3)` with `command_line: None`** — a block nobody
      submitted, minted by `Pump::unclaimed` because an empty line has no echo to claim one
      with. So "an empty submission has no verdict" cannot be asked of the submission: by the
      time the `D` arrives, the submission is not what the block belongs to.
    - **What that block is, is nothing at all.** No command line, and not one line of
      content — and Acter announces "command failed, exit code 1" about it.

    **The shape that survives**, and it is the one both facts point at: **a block that ends
    having had no command line and nothing printed into it has no verdict to announce.**
    Nothing was submitted, nothing was echoed, nothing was output; the `D` closing it is the
    shell restating `$?` on its way to the next prompt. Every other block keeps its verdict
    untouched — a submitted command that fails silently still has its line, and a block
    nobody submitted that *printed* something still has its content.

    The prompt after an empty Enter stays announced, and should: a block did start and end,
    so it is an ending rather than a repaint, and a terminal saying where you are after you
    press Enter is what a terminal does.

    **The rule, and what it is asked about.** `Pump` keeps one more fact about the block that
    is open — nobody submitted it, nothing named it, nothing has been printed into it — and a
    block closing in that state reports `SessionInput::NothingRan` instead of an exit code.
    The actor closes it exactly as it closes a finished one and says nothing. **The condition
    is who opened the block and never what it says**, which is what keeps a real command safe:
    a submission claims its block whether or not the shell's echo was recognised — busybox
    redrawing a wrapped line is the measured case — so a command that fails without printing a
    word keeps its verdict. Pinned from both sides, and the two guard tests pass with the rule
    and without it: `a_command_that_fails_silently_and_unrecognised_keeps_its_verdict` and
    `a_block_nobody_submitted_that_printed_something_keeps_its_verdict`.

    **A fourth `SessionInput` rather than an absent exit code**, which is B6 decision 8's own
    ruling applied again: `exit_code: Option<..>` was rejected there for re-overloading
    absence, and a missing code already means two other things. Sending `ExitCode(0)` was the
    worse of the two — 0 is the value that means the command succeeded.

    **Re-checked on the reader 2026-09-02**, NVDA 2026.1.1, `user` persona, live capture, at
    an integrated WSL Ubuntu with the setup accepted — integration confirmed on the spot by
    `false`, which announced "command failed, exit code 1". In both line modes: three empty
    Enters after a failure announce **the prompt and nothing else**, where each of them used
    to repeat the verdict. The user's own recipe, with `sleep 100` standing in for `gh`:
    `Ctrl+C` announced "command failed, exit code 130" once, and the Enters after it were
    quiet. And the halves that must not be lost all hold: a second `false` announces exit
    code 1 again, `ls /nope` announces its output and then exit code 2, and `echo hi` reads
    its output with no verdict.

    **One thing measured that is not this entry's, and it is worth an entry of its own.**
    `Ctrl+C` at an **idle** prompt still announces "command failed, exit code 130". The block
    is not barren there: `bash` echoes `^C` on the command line, so the block that opens is
    named and keeps its verdict. It says it once rather than at every Enter, so it is not the
    loop this entry closes — but "command failed" is a strange thing to hear when nothing was
    running, and A3.2 already owns what a listener should hear for a `Ctrl+C` that had
    nothing to stop.

30. **Closed 2026-09-02 — measured, and the answer went into 28.** This entry existed to
    find out whether a widget's selection is visible to a text diff at all, because a
    highlight drawn in colour alone would have needed attribute-aware diffing and a decision
    about how a "selected" row is expressed to a screen reader. Three prompt-driven samples
    answered it and none of them reopened anything, so what is left is the measurement note
    rather than an entry to build.

    - **`gh repo create`, 2026-08-31, gh 2.96.0**, aborted at the first prompt. No alternate
      screen. The selection is a `>` at the start of the row, with colour used as well as the
      marker rather than instead of it. Each arrow rewrites exactly two rows, and the engine
      reports exactly those two, because it emits only lines whose text changed. Both arrow
      encodings accepted; the selection wraps at both ends.
    - **`gh pr create`, 2026-09-02, same version**, aborted before anything was pushed. The
      same shape, and two findings nobody sought: the cursor is hidden for the whole prompt
      and parked below the list, so this is a rewritten-row rule and never a cursor-row one;
      and answering the prompt erases it — the question row became `? Where should we push
      the '…' branch? Cancel` and the three option rows became empty — which is what settles
      that the buffer applies revisions by id, blanks included, and keeps nothing else.
    - **PSReadLine's completion menu, 2026-09-02**, `pwsh` 7.6.5 with Tab bound to
      `MenuComplete`. **The selection is drawn in colour alone**, with no marker character
      anywhere, which is what a rule naming `>` would have been unable to hear — and it is
      why the rule is the row that gained non-whitespace content. The menu is also not where
      the answer is: each arrow rewrites the command line itself, one completion per press,
      so the anchored row already carries it. And the repaint is large — eleven line items on
      the first press — which is what settled that row count routes nothing.

    All three are written into DESIGN under "A row that changed is an answer", and the rule
    they produced shipped in 28 as `policies::far_end_row`. Nothing is owed here.
