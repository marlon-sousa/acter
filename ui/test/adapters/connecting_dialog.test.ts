// @vitest-environment jsdom
// Role: test — the dialog that holds a listener while a connection is being made.
//
// What it has to get right is small and was reported as a defect on 2026-08-30: Enter goes
// forward into something that says what is happening, rather than back onto the list of
// kinds. So these assert what it says, that it says it once however often it is asked, and
// that it can be taken away again — including after somebody has pressed Escape out of it,
// which is a thing they can do because an attempt in flight cannot be called off.

import { beforeEach, describe, expect, it } from 'vitest';

import { ConnectingDialog, connectingTo } from '../../src/adapters/connecting_dialog';

/** The static skeleton from views/main_window.html. */
const SKELETON = `
  <dialog id="connecting-dialog" aria-label="Connecting" aria-describedby="connecting-what">
    <h1>Connecting</h1>
    <p id="connecting-what"></p>
    <div data-live-region aria-live="polite" class="visually-hidden"></div>
  </dialog>
`;

let dialog: HTMLDialogElement;
let connecting: ConnectingDialog;

function byId<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

beforeEach(() => {
  document.body.innerHTML = SKELETON;
  dialog = byId<HTMLDialogElement>('connecting-dialog');
  // jsdom implements `<dialog>` only partially depending on version.
  dialog.showModal ??= function showModal(this: HTMLDialogElement) {
    this.open = true;
  };
  dialog.close ??= function close(this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event('close'));
  };
  connecting = new ConnectingDialog(dialog, byId('connecting-what'));
});

describe('while a connection is being made', () => {
  /** **In the words the connection uses when it succeeds**, so "connecting to WSL: Ubuntu"
   * and then "connected to WSL: Ubuntu" are one sentence about one thing. */
  it('says what is being connected to', () => {
    connecting.show('WSL: Ubuntu');

    expect(dialog.open).toBe(true);
    expect(byId('connecting-what').textContent).toBe('connecting to WSL: Ubuntu');
    expect(connectingTo('WSL: Ubuntu')).toBe('connecting to WSL: Ubuntu');
  });

  /** It is described by that sentence, which is what a reader speaks as it opens — the one
   * announcement this dialog gets for free, and the reason it needs nothing focusable. */
  it('is described by what it says', () => {
    expect(dialog.getAttribute('aria-describedby')).toBe('connecting-what');
  });

  /** **Nothing in it is a tab stop.** A paragraph is not a control (the rule the set-up
   * dialog was fixed under on the same day), and there is nothing here to press: an attempt
   * in flight cannot be called off. */
  it('puts nothing in the tab order', () => {
    connecting.show('Command Prompt');

    const stops = dialog.querySelectorAll(
      'button, input, select, textarea, [tabindex]:not([tabindex="-1"])',
    );
    expect(Array.from(stops)).toEqual([]);
  });

  /** Showing an open dialog throws `InvalidStateError`, silently, into a `void` call. */
  it('being shown again while it is open does nothing rather than throwing', () => {
    connecting.show('Command Prompt');

    expect(() => connecting.show('Command Prompt')).not.toThrow();
    expect(dialog.open).toBe(true);
  });

  it('goes away when the attempt is over', () => {
    connecting.show('Command Prompt');

    connecting.hide();

    expect(dialog.open).toBe(false);
  });

  /** **Escape is the platform's, and it is not a trap**: it leaves the attempt running and
   * puts the listener back on the Connect dialog underneath. So the attempt ending finds
   * this already closed, and must not throw over it. */
  it('is already closed when Escape got there first', () => {
    connecting.show('Command Prompt');
    dialog.close();

    expect(() => connecting.hide()).not.toThrow();
    expect(dialog.open).toBe(false);
  });

  /** And a second attempt after that one says its own far end. */
  it('says the next far end when it comes back', () => {
    connecting.show('Command Prompt');
    connecting.hide();

    connecting.show('WSL: Debian');

    expect(byId('connecting-what').textContent).toBe('connecting to WSL: Debian');
  });

  /** It carries a live region of its own, because everything under a modal is inert and the
   * backend's progress sentences arrive while this is the innermost dialog open. */
  it('has a live region for the progress sentences', () => {
    expect(dialog.querySelector('[data-live-region]')).not.toBeNull();
  });
});
