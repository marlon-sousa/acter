// Role: adapter (DOM) — what the window is: its titles, its status region, and which of
// its two faces it is showing.
//
// **Both titles are set explicitly, because one assignment does not do both.** A9 shipped
// believing that `document.title` in a Tauri window updates the native title as well; the
// user's NVDA said otherwise on 2026-08-25 — the report-title command still answered
// "Acter" while the document said "Acter - powershell". So the native title is a call
// through the shell port, and the document's is set here, and the two are set together in
// one method so they cannot drift.
//
// **The two faces are A10's.** With a session there is a terminal window: a results buffer
// and an edit field. With none there is a Connect button and nothing to type into — because
// a buffer holding nothing and a field that can submit nothing are two controls a listener
// has to arrow past to reach the only thing that would help them.

import type { WindowView } from '../ports/window_view';

/** What the window is called with no far end behind it. */
const PRODUCT = 'Acter';

/**
 * How long the window waits before placing focus for the first time.
 *
 * **Focus moved while the page is still loading does not take the reader's browse cursor
 * with it.** Measured with NVDA 2026.1.1 on 2026-08-26: the window opened with the Connect
 * button focused and announced as such, and the first Enter opened the *menu bar* — because
 * NVDA's virtual cursor was still at the top of a document it was in the middle of building,
 * and in browse mode Enter acts on the cursor rather than on the focus. The same keystroke
 * after any later focus change activates the button correctly.
 *
 * This is the same family as the announcer's own startup hold, and the same honesty applies:
 * the value is a starting point rather than a measurement. It wants tuning by ear, and this
 * comment corrected when somebody finds where the threshold actually sits.
 */
const STARTUP_HOLD_MS = 400;

/** The elements this adapter owns, named rather than ordered. */
export interface WindowElements {
  /** The document's `h1`, which says the same thing as the title bar. */
  heading: HTMLElement;
  /** The `role="status"` line in the footer. */
  statusRegion: HTMLElement;
  /** The edit field's form: the terminal window's half that takes input. */
  form: HTMLElement;
  /** What the window shows instead: the state, and the button that answers it. */
  notConnected: HTMLElement;
  /** Where focus belongs in each of the two faces. */
  editField: { focus(): void };
  connectButton: HTMLElement;
  document: Document;
  /** Sets the operating system's own window title. */
  setNativeTitle: (title: string) => void;
}

export class WindowChrome implements WindowView {
  /** Whether focus has yet been placed once. See STARTUP_HOLD_MS. */
  private opened = false;

  /**
   * `startupHold` is a parameter rather than a constant read directly, for the reason the
   * announcer's is: a test needs to be able to say "no hold" and go on asserting the focus
   * rules synchronously. Zero places focus immediately; production passes nothing.
   */
  constructor(
    private readonly elements: WindowElements,
    private readonly startupHold: number = STARTUP_HOLD_MS,
  ) {}

  /**
   * Put focus where it belongs in the face that is showing: the edit field with a session,
   * the Connect button without one.
   *
   * **This is what the menu bar and the dialogs return to.** They used to return to the edit
   * field by name, which was right while there was always one — and since A10 there is not.
   * Measured with NVDA on 2026-08-26: Escape out of the menu bar in an unconnected window
   * focused a hidden input, which does nothing, and left the listener stranded on a menu
   * item they had just closed.
   */
  focus(): void {
    if (this.elements.form.hidden) {
      this.elements.connectButton.focus();
    } else {
      this.elements.editField.focus();
    }
  }

  connectedTo(name: string | null): void {
    const title = name === null ? PRODUCT : `${PRODUCT} - ${name}`;
    // Three places, one value: the native title bar, the document, and the heading.
    this.elements.setNativeTitle(title);
    this.elements.document.title = title;
    this.elements.heading.textContent = title;
  }

  status(text: string): void {
    // Written only when it changes. A live region that is reassigned the same text can
    // still fire an accessibility event, and a status that repeats itself for no reason is
    // a status a listener learns to ignore.
    if (this.elements.statusRegion.textContent !== text) {
      this.elements.statusRegion.textContent = text;
    }
  }

  /**
   * Show the terminal window, or show that there is no session to show one for.
   *
   * **The results buffer is not touched here**, and that is the whole of the disconnect
   * rule: it appears with its first content and stays afterwards, because once a session
   * has ended the buffer is the record of what happened and a user who typed `exit` by
   * accident must not lose it. Only the edit field goes, since there is nothing left to
   * submit to.
   *
   * **Focus is rescued, never stolen.** Hiding the element focus is inside strands it on
   * the document body, where a listener has nothing under them and no obvious way back —
   * so focus moves into whatever is now showing, but *only* if it was in what just went
   * away or nowhere at all. A user reading the buffer when their shell exits keeps their
   * place.
   */
  showTerminal(live: boolean): void {
    const { form, notConnected, document } = this.elements;
    const active = document.activeElement;
    const stranded =
      active === null ||
      active === document.body ||
      form.contains(active) ||
      notConnected.contains(active);

    form.hidden = !live;
    notConnected.hidden = live;

    if (!stranded) {
      return;
    }
    // The first placement waits for the reader to finish building its view of the document;
    // every later one is a focus change in a page that is already there, and must not lag.
    if (this.opened || this.startupHold === 0) {
      this.opened = true;
      this.focus();
      return;
    }
    this.opened = true;
    setTimeout(() => this.focus(), this.startupHold);
  }
}
