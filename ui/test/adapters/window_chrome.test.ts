// @vitest-environment jsdom
// Role: test — the window's two titles and its status region.
//
// What matters here is that the titles cannot disagree: they are set from one value, so a
// test that checks only one of them would pass while the other said something else.

import { beforeEach, describe, expect, it } from 'vitest';

import { WindowChrome } from '../../src/adapters/window_chrome';

let heading: HTMLElement;
let status: HTMLElement;
let chrome: WindowChrome;
/** What the operating system's title bar was told, in order. */
let native: string[];

beforeEach(() => {
  document.body.innerHTML = `
    <h1 id="window-title">Acter</h1>
    <p id="connection-status" role="status">connecting</p>
  `;
  heading = document.getElementById('window-title') as HTMLElement;
  status = document.getElementById('connection-status') as HTMLElement;
  native = [];
  chrome = new WindowChrome(heading, status, document, (title) => native.push(title));
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
