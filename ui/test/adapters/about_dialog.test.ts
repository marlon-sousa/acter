// @vitest-environment jsdom
// Role: test — the About dialog: what it reads out, and where focus is before and after.

import { beforeEach, describe, expect, it } from 'vitest';

import { AboutDialog } from '../../src/adapters/about_dialog';
import type { AboutFacts, AppShell } from '../../src/ports/app_shell';

const SKELETON = `
  <dialog id="about-dialog" aria-labelledby="about-title">
    <h1 id="about-title">About Acter</h1>
    <p id="about-name"></p>
    <p id="about-version"></p>
    <p id="about-copyright"></p>
    <p id="about-licence"></p>
    <p id="about-settings"></p>
    <button id="about-close" type="button">Close</button>
  </dialog>
  <input id="command-input" />
`;

class StubShell implements AppShell {
  asked = 0;

  about(): Promise<AboutFacts> {
    this.asked += 1;
    return Promise.resolve({
      name: 'Acter',
      version: '9.9.9-from-the-build',
      version_said: 'Version 9.9.9-from-the-build.',
      copyright: '© 2026 Marlon Brandão de Sousa',
      licence: 'MIT',
      settings_folder: 'D:\\portable\\acter\\settings',
      settings_standing:
        'Acter is running portable, so its settings are kept beside the program.',
    });
  }

  setTitle(): Promise<void> {
    return Promise.resolve();
  }

  connection(): Promise<string | null> {
    return Promise.resolve('powershell');
  }

  platform(): Promise<string> {
    return Promise.resolve('windows');
  }

  exit(): Promise<void> {
    return Promise.resolve();
  }
}

let shell: StubShell;
let dialog: HTMLDialogElement;
let returned: number;

function byId<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) {
    throw new Error(`missing element: ${id}`);
  }
  return element as T;
}

function open(): Promise<void> {
  const about = new AboutDialog(dialog, shell, {
    focus: () => {
      returned += 1;
      byId('command-input').focus();
    },
  });
  return about.open();
}

function said(id: string): string {
  return byId(id).textContent ?? '';
}

beforeEach(() => {
  document.body.innerHTML = SKELETON;
  shell = new StubShell();
  dialog = byId<HTMLDialogElement>('about-dialog');
  returned = 0;
  // Some jsdom versions lack `showModal` and `close` on `<dialog>`, so every dialog test
  // patches them in when absent.
  dialog.showModal ??= function showModal(this: HTMLDialogElement) {
    this.open = true;
  };
  dialog.close ??= function close(this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event('close'));
  };
});

describe('what it says', () => {
  it('reads its four facts from the build rather than from the page', async () => {
    await open();

    expect(shell.asked).toBe(1);
    expect(said('about-name')).toBe('Acter');
    expect(said('about-version')).toContain('9.9.9-from-the-build');
    expect(said('about-copyright')).toContain('Marlon Brandão de Sousa');
    expect(said('about-licence')).toContain('MIT');
  });

  it('says the version and the licence as words, not as bare values', async () => {
    await open();

    expect(said('about-version')).toBe(
      'Version 9.9.9-from-the-build. 9.9.9-from-the-build',
    );
    expect(said('about-licence')).toBe('MIT licence');
  });

  it('is open once it has been asked to open', async () => {
    await open();

    expect(dialog.open).toBe(true);
  });
});

describe('where focus is', () => {
  it('tab stays inside a dialog that has a single control', async () => {
    await open();
    const close = byId('about-close');
    close.focus();

    close.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }),
    );

    expect(document.activeElement?.id).toBe('about-close');
  });

  it('shift tab stays inside it too', async () => {
    await open();
    const close = byId('about-close');
    close.focus();

    close.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'Tab',
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(document.activeElement?.id).toBe('about-close');
  });

  it('closing it puts focus in the edit field', async () => {
    await open();

    dialog.close();

    expect(returned).toBe(1);
    expect(document.activeElement?.id).toBe('command-input');
  });

  it('the close button closes it', async () => {
    await open();

    byId('about-close').dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(dialog.open).toBe(false);
    expect(returned).toBe(1);
  });
});
