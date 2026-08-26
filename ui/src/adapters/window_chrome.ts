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
// **The two windows are A10's**, and they are swapped as units rather than assembled from
// controls that wink in and out. A window with no session and a window with one are
// different things: the first holds a Connect button and nothing else, because a buffer
// holding nothing and a field that can submit nothing are two controls a listener has to
// arrow past to reach the only thing that would help them. The second holds the terminal —
// and once its session ends it keeps the buffer, which is by then the record of what
// happened, and replaces the edit field with a Connect button of its own.
//
// Exactly one of the two is in the document at any moment. That exclusivity is the point:
// three independent toggles is one window changing shape, which is much harder to learn by
// ear than two windows you are moved between.

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
  /** The whole window shown before anything has been connected to. */
  notConnectedWindow: HTMLElement;
  /** Its Connect button, which is where focus lands when that window opens. */
  connectButton: HTMLElement;
  /** The whole window shown from the first connection onward. */
  terminalWindow: HTMLElement;
  /** The edit field's form: the terminal window's half that takes input. */
  form: HTMLElement;
  /** Where focus belongs while a session is live. */
  editField: { focus(): void };
  /** What the terminal window shows in place of the form once its session has ended. */
  ended: HTMLElement;
  /** And that block's own Connect button. */
  reconnectButton: HTMLElement;
  document: Document;
  /** Sets the operating system's own window title. */
  setNativeTitle: (title: string) => void;
}

export class WindowChrome implements WindowView {
  /** Whether focus has yet been placed once. See STARTUP_HOLD_MS. */
  private opened = false;
  /** Whether a session has ever run in this window; see `showTerminal`. */
  private hasConnected = false;

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
    this.landing().focus();
  }

  /** Whichever control the window that is showing keeps focus on. */
  private landing(): { focus(): void } {
    if (this.elements.terminalWindow.hidden) {
      return this.elements.connectButton;
    }
    return this.elements.ended.hidden
      ? this.elements.editField
      : this.elements.reconnectButton;
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
   * submit to — replaced in place by the line and button that say so.
   *
   * The empty window never comes back once a session has run, for the same reason: it holds
   * nothing, and swapping to it would take the transcript off the screen.
   *
   * **Focus is rescued, never stolen.** Hiding the element focus is inside strands it on
   * the document body, where a listener has nothing under them and no obvious way back —
   * so focus moves into whatever is now showing, but *only* if it was in what just went
   * away or nowhere at all. A user reading the buffer when their shell exits keeps their
   * place.
   */
  showTerminal(live: boolean): void {
    const { notConnectedWindow, terminalWindow, form, ended, document } = this.elements;
    const active = document.activeElement;
    const stranded =
      active === null ||
      active === document.body ||
      notConnectedWindow.contains(active) ||
      form.contains(active) ||
      ended.contains(active);

    // Once a session has existed this window is a terminal window for good: what it holds
    // afterwards is the record of what happened, and going back to the empty window would
    // throw it away.
    this.hasConnected = this.hasConnected || live;

    notConnectedWindow.hidden = this.hasConnected;
    terminalWindow.hidden = !this.hasConnected;
    // Inside the terminal window, the session's own state: the edit field while it is live,
    // and the line and button that answer it once it is not.
    form.hidden = !live;
    ended.hidden = live;

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
