// @vitest-environment jsdom
// Role: test — the Help dialog: that it opens, that it can be left, and that the topic
// inside it is shaped to be read rather than to be operated.

import { beforeEach, describe, expect, it } from 'vitest';

import { HelpDialog } from '../../src/adapters/help_dialog';

const SKELETON = `
  <dialog id="help-dialog" aria-labelledby="help-title" aria-describedby="help-summary">
    <h1 id="help-title">Acter help</h1>
    <p id="help-summary">Five short sections: what Acter is, moving around the window, connecting to a shell, sessions Acter has set up, and the dialog that asks. Use your reader's heading key to move between them.</p>
    <h2 id="help-what-acter-is" tabindex="-1">What Acter is</h2>
    <p>Acter is a terminal you use by listening. You type a command, it runs in a shell, and Acter reads out what the command prints.</p>
    <h2>Moving around the window</h2>
    <p>The command line is where you type. F6 moves between it and the results area.</p>
    <h2>Connecting to a shell</h2>
    <p>A window with nothing running has one button, Connect.</p>
    <h2 id="help-setting-up" tabindex="-1">Sessions Acter has set up, and sessions it has not</h2>
    <p>A session where that has been done is called an integrated session; one where it has not is called an unintegrated session.</p>
    <h2>The dialog that asks to set a session up</h2>
    <p>Run command runs it. Skip leaves the session as it is.</p>
    <button id="help-close" type="button">Close</button>
  </dialog>
  <input id="command-input" />
`;

let dialog: HTMLDialogElement;
let returned: number;
let help: HelpDialog;

function byId<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) {
    throw new Error(`missing element: ${id}`);
  }
  return element as T;
}

beforeEach(() => {
  document.body.innerHTML = SKELETON;
  dialog = byId<HTMLDialogElement>('help-dialog');
  returned = 0;
  dialog.showModal ??= function showModal(this: HTMLDialogElement) {
    this.open = true;
  };
  dialog.close ??= function close(this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event('close'));
  };
  help = new HelpDialog(dialog, {
    focus: () => {
      returned += 1;
      byId('command-input').focus();
    },
  });
});

describe('opening it', () => {
  it('opens', () => {
    help.open();

    expect(dialog.open).toBe(true);
  });

  it('lands on the first section when nobody asked for one', () => {
    help.open();

    expect(document.activeElement?.id).toBe('help-what-acter-is');
  });

  it('being asked again while it is open does nothing rather than throwing', () => {
    help.open();

    expect(() => help.open()).not.toThrow();
    expect(dialog.open).toBe(true);
  });
});

describe('opening it at a section', () => {
  /** Asserted synchronously: focus must be on the heading in the same turn the dialog opens. */
  it('puts focus on the heading that was asked for, as the dialog opens', () => {
    help.open({ topic: 'help-setting-up' });

    expect(document.activeElement?.id).toBe('help-setting-up');
  });

  it('leaves that heading out of the tab order', () => {
    expect(byId('help-setting-up').tabIndex).toBe(-1);
  });

  it('comes back to whoever opened it rather than to the window', () => {
    const button = byId('help-close');
    let cameBack = 0;
    help.open({ topic: 'help-setting-up', returnTo: { focus: () => (cameBack += 1) } });

    dialog.close();

    expect(cameBack).toBe(1);
    expect(returned).toBe(0);
    expect(button).not.toBeNull();
  });

  it('goes back to the window again when nobody else asks for it', () => {
    help.open({ topic: 'help-setting-up', returnTo: { focus: () => {} } });
    dialog.close();

    help.open();
    dialog.close();

    expect(returned).toBe(1);
  });
});

describe('leaving it', () => {
  it('closing it hands focus back to the window', () => {
    help.open();

    dialog.close();

    expect(returned).toBe(1);
    expect(document.activeElement?.id).toBe('command-input');
  });

  it('the close button closes it', () => {
    help.open();

    byId('help-close').dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(dialog.open).toBe(false);
    expect(returned).toBe(1);
  });

  it('tab stays inside it rather than dropping into the document', () => {
    help.open();
    const close = byId('help-close');
    close.focus();

    close.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }),
    );

    expect(document.activeElement?.id).toBe('help-close');
  });
});

describe('the topic is shaped to be read', () => {
  it('has no application region anywhere in it', () => {
    expect(dialog.querySelector('[role="application"]')).toBeNull();
    expect(dialog.getAttribute('role')).toBeNull();
  });

  it('says one short line when it opens rather than reading itself out', () => {
    const describedBy = dialog.getAttribute('aria-describedby');
    expect(describedBy).not.toBeNull();

    const summary = dialog.querySelector(`#${describedBy}`);
    expect(summary).not.toBeNull();
    expect(summary?.tagName).toBe('P');
    expect((summary?.textContent ?? '').length).toBeLessThan(200);
  });

  it('is broken into headings under its title', () => {
    expect(dialog.querySelector('h1')?.id).toBe('help-title');
    expect(dialog.querySelectorAll('h2').length).toBeGreaterThanOrEqual(2);
  });

  it('says how many sections there are, and has that many', () => {
    const summary = dialog.querySelector('#help-summary')?.textContent ?? '';

    expect(summary).toContain('Five short sections');
    expect(dialog.querySelectorAll('h2')).toHaveLength(5);
  });

  it('explains without using the words a listener does not have', () => {
    const text = (dialog.textContent ?? '').toLowerCase();

    for (const jargon of ['osc', 'marker', 'verdict', 'exit code']) {
      expect(text, `"${jargon}" is this project's word, not a user's`).not.toContain(
        jargon,
      );
    }
  });

  it('says what an integrated session is before calling one that', () => {
    const text = dialog.textContent ?? '';

    expect(text).toContain('is called an integrated session');
  });
});
