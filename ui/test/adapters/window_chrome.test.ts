// @vitest-environment jsdom
// Role: test — the window's two titles, its status region, and which of its two faces it is
// showing.

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
let farEndInput: HTMLElement;
let connectButton: HTMLElement;
let reconnectButton: HTMLElement;
let chrome: WindowChrome;
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
      <div id="far-end-line" hidden><span id="far-end-input" tabindex="0"></span></div>
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
  farEndInput = byId('far-end-input');
  connectButton = byId('connect-button');
  reconnectButton = byId('reconnect-button');
  native = [];
  chrome = new WindowChrome(
    {
      heading,
      statusRegion: status,
      notConnectedWindow: notConnected,
      connectButton,
      terminalWindow: terminal,
      form,
      editField: { focus: () => input.focus() },
        farEndField: { focus: () => farEndInput.focus() },
      ended,
      reconnectButton,
      document,
      setNativeTitle: (title: string) => native.push(title),
    },
    0,
  );
});

describe('what the window is called', () => {
  it('lands on the program line while the local form is hidden', () => {
    chrome.showTerminal(true);
    chrome.showLocalLine(false);

    chrome.focus();

    expect(document.activeElement).toBe(farEndInput);
  });

  it("lands on Acter's line when that is the one showing", () => {
    chrome.showTerminal(true);
    chrome.showLocalLine(true);

    chrome.focus();

    expect(document.activeElement).toBe(input);
  });

  it('names the far end in the native title, the document and the heading', () => {
    chrome.connectedTo('PowerShell');

    expect(native).toEqual(['Acter - PowerShell']);
    expect(document.title).toBe('Acter - PowerShell');
    expect(heading.textContent).toBe('Acter - PowerShell');
  });

  it('is the product alone when nothing is connected', () => {
    chrome.connectedTo('PowerShell');
    chrome.connectedTo(null);

    expect(native.at(-1)).toBe('Acter');
    expect(document.title).toBe('Acter');
    expect(heading.textContent).toBe('Acter');
  });

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

  it('stays on the terminal window when the session ends, and swaps the edit field out', () => {
    chrome.showTerminal(true);

    chrome.showTerminal(false);

    expect(notConnected.hidden).toBe(true);
    expect(terminal.hidden).toBe(false);
    expect(form.hidden).toBe(true);
    expect(ended.hidden).toBe(false);
  });

  it('never touches the results buffer', () => {
    const results = byId('results');
    results.hidden = false;

    chrome.showTerminal(false);

    expect(results.hidden).toBe(false);
  });

  it('moves focus to the Connect button when the session ends', () => {
    chrome.showTerminal(true);
    results.hidden = false;
    input.focus();

    chrome.showTerminal(false);

    expect(document.activeElement).toBe(reconnectButton);
  });

  it('moves focus to the Connect button when there is no transcript either', () => {
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

  it('places focus when there was none', () => {
    chrome.showTerminal(false);

    expect(document.activeElement).toBe(connectButton);
  });

  it('leaves focus alone when it is somewhere else', () => {
    chrome.showTerminal(true);
    heading.tabIndex = -1;
    heading.focus();

    chrome.showTerminal(false);

    expect(document.activeElement).toBe(heading);
  });
});

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

  it('returns to the ended terminal window button after a session has run', () => {
    chrome.showTerminal(true);
    chrome.showTerminal(false);
    heading.tabIndex = -1;
    heading.focus();

    chrome.focus();

    expect(document.activeElement).toBe(reconnectButton);
  });

  it('returns to that button when there is a transcript as well', () => {
    chrome.showTerminal(true);
    results.hidden = false;
    chrome.showTerminal(false);
    heading.tabIndex = -1;
    heading.focus();

    chrome.focus();

    expect(document.activeElement).toBe(reconnectButton);
  });
});

describe('the startup hold', () => {
  function held(): WindowChrome {
    return new WindowChrome(
      {
        heading,
        statusRegion: status,
        notConnectedWindow: notConnected,
        connectButton,
        terminalWindow: terminal,
        form,
        editField: { focus: () => input.focus() },
        farEndField: { focus: () => farEndInput.focus() },
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

  it('leaves focus alone when the user moved it somewhere else during the hold', async () => {
    const chrome = held();
    chrome.showTerminal(true);
    heading.tabIndex = -1;
    heading.focus();

    await new Promise((resolve) => setTimeout(resolve, 40));

    expect(document.activeElement).toBe(heading);
  });

  it('places every later one immediately', async () => {
    const chrome = held();
    chrome.showTerminal(false);
    await new Promise((resolve) => setTimeout(resolve, 40));

    chrome.showTerminal(true);

    expect(document.activeElement).toBe(input);
  });
});
