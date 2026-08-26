// @vitest-environment jsdom
// Role: test — the window's two titles, its status region, and which of its two faces it is
// showing.
//
// What matters for the titles is that they cannot disagree: they are set from one value, so
// a test that checks only one of them would pass while the other said something else. What
// matters for the faces is that focus is never stranded (spec A10).

import { beforeEach, describe, expect, it } from 'vitest';

import { WindowChrome } from '../../src/adapters/window_chrome';

let heading: HTMLElement;
let status: HTMLElement;
let form: HTMLElement;
let notConnected: HTMLElement;
let terminal: HTMLElement;
let results: HTMLElement;
let ended: HTMLElement;
let input: HTMLElement;
let connectButton: HTMLElement;
let reconnectButton: HTMLElement;
let chrome: WindowChrome;
/** What the operating system's title bar was told, in order. */
let native: string[];

function byId(id: string): HTMLElement {
  return document.getElementById(id) as HTMLElement;
}

beforeEach(() => {
  document.body.innerHTML = `
    <h1 id="window-title">Acter</h1>
    <div id="not-connected-window">
      <p>Not connected.</p>
      <button id="connect-button">Connect</button>
    </div>
    <div id="terminal-window" hidden>
      <div id="results" hidden></div>
      <form id="command-form"><input id="command-input" /></form>
      <div id="terminal-ended" hidden>
        <button id="reconnect-button">Connect</button>
      </div>
    </div>
    <p id="connection-status" role="status">connecting</p>
  `;
  heading = byId('window-title');
  status = byId('connection-status');
  form = byId('command-form');
  notConnected = byId('not-connected-window');
  terminal = byId('terminal-window');
  results = byId('results');
  results.tabIndex = -1;
  ended = byId('terminal-ended');
  input = byId('command-input');
  connectButton = byId('connect-button');
  reconnectButton = byId('reconnect-button');
  native = [];
  // No startup hold: the hold is a reader-timing measurement of its own, asserted in its
  // own test, and every other rule here is about *where* focus goes rather than when.
  chrome = new WindowChrome(
    {
      heading,
      statusRegion: status,
      notConnectedWindow: notConnected,
      connectButton,
      terminalWindow: terminal,
      results,
      buffer: { focus: () => results.focus() },
      form,
      editField: { focus: () => input.focus() },
      ended,
      reconnectButton,
      document,
      setNativeTitle: (title: string) => native.push(title),
    },
    0,
  );
});

describe('what the window is called', () => {
  /** All three, from one value. The native title is the one the desktop reads out in the
   * task switcher, and it is the one A9 shipped without: assigning `document.title` in a
   * Tauri window leaves the native title alone, which the user's NVDA reported on
   * 2026-08-25 while the document said something else. */
  it('names the far end in the native title, the document and the heading', () => {
    chrome.connectedTo('PowerShell');

    expect(native).toEqual(['Acter - PowerShell']);
    expect(document.title).toBe('Acter - PowerShell');
    expect(heading.textContent).toBe('Acter - PowerShell');
  });

  /** With nothing behind it the window is just the product, in both places. */
  it('is the product alone when nothing is connected', () => {
    chrome.connectedTo('PowerShell');
    chrome.connectedTo(null);

    expect(native.at(-1)).toBe('Acter');
    expect(document.title).toBe('Acter');
    expect(heading.textContent).toBe('Acter');
  });

  /** The names come from the connect list, so they arrive with spaces and punctuation in
   * them; nothing here may mangle one. */
  it('passes a name through exactly as it was given', () => {
    chrome.connectedTo('WSL: Ubuntu');

    expect(document.title).toBe('Acter - WSL: Ubuntu');
    expect(heading.textContent).toBe('Acter - WSL: Ubuntu');
  });
});

describe('the status region', () => {
  it('says what it was told', () => {
    chrome.status('connected');

    expect(status.textContent).toBe('connected');
  });

  /** A live region reassigned the same text can still fire an accessibility event, and a
   * status that repeats itself for no reason is one a listener learns to ignore. */
  it('does not rewrite itself with text it already says', () => {
    chrome.status('connected');
    const first = status.firstChild;

    chrome.status('connected');

    expect(status.firstChild).toBe(first);
  });

  it('rewrites itself when the state really changed', () => {
    chrome.status('connecting');
    chrome.status('connected');

    expect(status.textContent).toBe('connected');
  });
});

// **The window's two faces** (spec A10). With a session there is a terminal window: a
// results buffer and an edit field. With none there is a Connect button and nothing to type
// into, because a field that can submit nothing is a control a listener has to arrow past to
// reach the only thing that would help them.
describe('which face the window shows', () => {
  it('opens on the empty window, with no terminal at all', () => {
    chrome.showTerminal(false);

    expect(notConnected.hidden).toBe(false);
    expect(terminal.hidden).toBe(true);
  });

  it('swaps to the terminal window when a session starts', () => {
    chrome.showTerminal(true);

    expect(notConnected.hidden).toBe(true);
    expect(terminal.hidden).toBe(false);
    expect(form.hidden).toBe(false);
    expect(ended.hidden).toBe(true);
  });

  /** **The two windows are exclusive**, and once a session has run the empty one never
   * comes back: it holds nothing, and swapping to it would take the transcript off the
   * screen. What answers a session ending is the terminal window's own ended state. */
  it('stays on the terminal window when the session ends, and swaps the edit field out', () => {
    chrome.showTerminal(true);

    chrome.showTerminal(false);

    expect(notConnected.hidden).toBe(true);
    expect(terminal.hidden).toBe(false);
    expect(form.hidden).toBe(true);
    expect(ended.hidden).toBe(false);
  });

  /** The disconnect rule, and the reason it is not in this method: the buffer is the record
   * of a session that ended, and a user who typed `exit` by accident must not lose it. */
  it('never touches the results buffer', () => {
    const results = byId('results');
    results.hidden = false;

    chrome.showTerminal(false);

    expect(results.hidden).toBe(false);
  });

  /** **Focus is rescued, never stolen.** Hiding the element focus is inside strands it on
   * the document body, where a listener has nothing under them and no obvious way back.
   *
   * **A session that has ended leaves a transcript, and reading it is what a user does
   * next.** The user met the opposite on 2026-08-26: focus on the Connect button, and the
   * history they had just been told was kept was not where they were. */
  it('moves focus into the transcript when the session ends', () => {
    chrome.showTerminal(true);
    results.hidden = false;
    input.focus();

    chrome.showTerminal(false);

    expect(document.activeElement).toBe(results);
  });

  /** With nothing in the buffer there is nothing to land in, so the button it is. */
  it('moves focus to the Connect button when there is no transcript', () => {
    chrome.showTerminal(true);
    input.focus();

    chrome.showTerminal(false);

    expect(document.activeElement).toBe(reconnectButton);
  });

  it('moves focus into the edit field when the terminal window comes back', () => {
    chrome.showTerminal(false);
    connectButton.focus();

    chrome.showTerminal(true);

    expect(document.activeElement).toBe(input);
  });

  /** A window opening with focus nowhere is the launch case, and it must land somewhere. */
  it('places focus when there was none', () => {
    chrome.showTerminal(false);

    expect(document.activeElement).toBe(connectButton);
  });

  /** But a user reading the buffer when their shell exits keeps their place: focus was not
   * in what went away, so nothing moves it. */
  it('leaves focus alone when it is somewhere else', () => {
    chrome.showTerminal(true);
    heading.tabIndex = -1;
    heading.focus();

    chrome.showTerminal(false);

    expect(document.activeElement).toBe(heading);
  });
});

// **Where the menu bar and the dialogs come back to.** They returned to the edit field by
// name, which was right while there was always one — and since A10 there is not. Measured
// with NVDA on 2026-08-26: Escape out of the menu bar in an unconnected window focused a
// hidden input, which does nothing at all, and left the listener stranded on a menu item
// they had just closed.
describe('coming back to the window', () => {
  it('returns to the edit field when a session is showing', () => {
    chrome.showTerminal(true);
    connectButton.focus();

    chrome.focus();

    expect(document.activeElement).toBe(input);
  });

  it('returns to the Connect button when none is', () => {
    chrome.showTerminal(false);
    heading.tabIndex = -1;
    heading.focus();

    chrome.focus();

    expect(document.activeElement).toBe(connectButton);
  });

  /** And to the terminal window's own button once a session has run there, because that is
   * the window the listener is in. */
  it('returns to the ended terminal window button after a session has run', () => {
    chrome.showTerminal(true);
    chrome.showTerminal(false);
    heading.tabIndex = -1;
    heading.focus();

    chrome.focus();

    expect(document.activeElement).toBe(reconnectButton);
  });

  it('returns into the transcript when there is one to read', () => {
    chrome.showTerminal(true);
    results.hidden = false;
    chrome.showTerminal(false);
    heading.tabIndex = -1;
    heading.focus();

    chrome.focus();

    expect(document.activeElement).toBe(results);
  });
});

// **Focus moved while the page is still loading does not take the reader's browse cursor
// with it** — measured with NVDA on 2026-08-26, where the window opened with the Connect
// button focused and the first Enter opened the menu bar instead of pressing it. So the
// first placement waits; every later one must not.
describe('the startup hold', () => {
  function held(): WindowChrome {
    return new WindowChrome(
      {
        heading,
        statusRegion: status,
        notConnectedWindow: notConnected,
        connectButton,
        terminalWindow: terminal,
        results,
        buffer: { focus: () => results.focus() },
        form,
        editField: { focus: () => input.focus() },
        ended,
        reconnectButton,
        document,
        setNativeTitle: () => {},
      },
      20,
    );
  }

  it('defers the first placement and does it once the hold is up', async () => {
    const chrome = held();

    chrome.showTerminal(false);
    expect(document.activeElement).not.toBe(connectButton);

    await new Promise((resolve) => setTimeout(resolve, 40));
    expect(document.activeElement).toBe(connectButton);
  });

  /** A disconnect mid-session must move focus at once: the field the user was typing in has
   * just gone, and a listener with focus on nothing is exactly what the rescue exists for. */
  it('places every later one immediately', async () => {
    const chrome = held();
    chrome.showTerminal(false);
    await new Promise((resolve) => setTimeout(resolve, 40));

    chrome.showTerminal(true);

    expect(document.activeElement).toBe(input);
  });
});
