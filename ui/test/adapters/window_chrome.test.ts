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

beforeEach(() => {
  document.body.innerHTML = `
    <h1 id="window-title">Acter</h1>
    <p id="connection-status" role="status">connecting</p>
  `;
  heading = document.getElementById('window-title') as HTMLElement;
  status = document.getElementById('connection-status') as HTMLElement;
  chrome = new WindowChrome(heading, status, document);
});

describe('what the window is called', () => {
  it('names the far end in both titles at once', () => {
    chrome.connectedTo('PowerShell');

    expect(document.title).toBe('Acter - PowerShell');
    expect(heading.textContent).toBe('Acter - PowerShell');
  });

  /** With nothing behind it the window is just the product, in both places. */
  it('is the product alone when nothing is connected', () => {
    chrome.connectedTo('PowerShell');
    chrome.connectedTo(null);

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
