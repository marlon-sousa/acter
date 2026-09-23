// @vitest-environment jsdom
// Role: test — the dialog that holds a listener while a connection is being made.

import { beforeEach, describe, expect, it } from 'vitest';

import { ConnectingDialog, connectingTo } from '../../src/adapters/connecting_dialog';

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
  it('says what is being connected to', () => {
    connecting.show('WSL: Ubuntu');

    expect(dialog.open).toBe(true);
    expect(byId('connecting-what').textContent).toBe('connecting to WSL: Ubuntu');
    expect(connectingTo('WSL: Ubuntu')).toBe('connecting to WSL: Ubuntu');
  });

  it('is described by what it says', () => {
    expect(dialog.getAttribute('aria-describedby')).toBe('connecting-what');
  });

  it('puts nothing in the tab order', () => {
    connecting.show('Command Prompt');

    const stops = dialog.querySelectorAll(
      'button, input, select, textarea, [tabindex]:not([tabindex="-1"])',
    );
    expect(Array.from(stops)).toEqual([]);
  });

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

  it('is already closed when Escape got there first', () => {
    connecting.show('Command Prompt');
    dialog.close();

    expect(() => connecting.hide()).not.toThrow();
    expect(dialog.open).toBe(false);
  });

  it('says the next far end when it comes back', () => {
    connecting.show('Command Prompt');
    connecting.hide();

    connecting.show('WSL: Debian');

    expect(byId('connecting-what').textContent).toBe('connecting to WSL: Debian');
  });

  it('has a live region for the progress sentences', () => {
    expect(dialog.querySelector('[data-live-region]')).not.toBeNull();
  });
});
