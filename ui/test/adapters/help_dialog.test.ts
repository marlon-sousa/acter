// @vitest-environment jsdom
// Role: test — the Help dialog: that it opens, that it can be left, and that the topic
// inside it is shaped to be read rather than to be operated.
//
// The platform's three behaviours (announced as a dialog, focus trapped while open,
// Escape closes) are not restated here, for the About suite's reason: asserting that a
// `<dialog>` is a dialog tests jsdom. What is Acter's is below — and one of these is not
// about behaviour at all but about the markup, because A13's whole argument is that this
// dialog must stay readable when every other dialog added since B9 is not.

import { beforeEach, describe, expect, it } from 'vitest';

import { HelpDialog } from '../../src/adapters/help_dialog';

/** The static skeleton from views/main_window.html, restated so this suite tests the
 * structure the product ships rather than one invented for the test. */
const SKELETON = `
  <dialog id="help-dialog" aria-labelledby="help-title" aria-describedby="help-summary">
    <h1 id="help-title">Acter help</h1>
    <p id="help-summary">Three short sections about what you hear when you run a command. Use your reader's heading key to move between them.</p>
    <h2>What you always hear</h2>
    <p>When you run a command, Acter reads out what the command prints.</p>
    <h2>What you sometimes do not hear</h2>
    <p>Acter cannot see whether a command worked. It has to be told, by the shell.</p>
    <h2>Which sessions are which</h2>
    <p>Sessions on this computer set themselves up when they start.</p>
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
  // jsdom implements `<dialog>` only partially depending on version; these two keep the
  // suite about Acter's behaviour rather than about jsdom's coverage of the element.
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

  /** It has two ways in — F1 and the menu item — so being asked while already open is an
   * ordinary thing rather than a double press, and `showModal` on an open dialog throws
   * `InvalidStateError` into a `void` call where nobody sees it. */
  it('being asked again while it is open does nothing rather than throwing', () => {
    help.open();

    expect(() => help.open()).not.toThrow();
    expect(dialog.open).toBe(true);
  });
});

describe('leaving it', () => {
  /** It cannot return focus to whatever opened it: F1 is a key, and the menu item has
   * closed by then. So the destination is stated rather than remembered, and it is
   * "whatever this window is showing" rather than the edit field by name, because since
   * A10 there is not always one. */
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
  /** **The assertion A13 exists for.** Every dialog added since B9 wraps itself in
   * `role="application"` so the arrows reach the widget; inside one, prose cannot be
   * arrowed, which the Connect dialog's own note records. A help topic a listener cannot
   * arrow through line by line is not help, so this dialog must never gain that wrapper —
   * and an edit that added one would otherwise look harmless and consistent. */
  it('has no application region anywhere in it', () => {
    expect(dialog.querySelector('[role="application"]')).toBeNull();
    expect(dialog.getAttribute('role')).toBeNull();
  });

  /** **Measured with NVDA on 2026-08-27, before this existed.** Without a description of
   * its own the dialog announced its title and then read the whole topic in one utterance
   * — six paragraphs, headings left out, because a reader with nothing else to speak falls
   * back to the content. So the one read a listener gets for free was the wall of prose,
   * and the part it dropped was the structure built for skimming.
   *
   * The description is what the host-key dialog already uses for the same reason (spec B9),
   * and it must point at something short. A test that only asserted the attribute exists
   * would pass if it pointed at the whole body, which is the bug. */
  it('says one short line when it opens rather than reading itself out', () => {
    const describedBy = dialog.getAttribute('aria-describedby');
    expect(describedBy).not.toBeNull();

    const summary = dialog.querySelector(`#${describedBy}`);
    expect(summary).not.toBeNull();
    expect(summary?.tagName).toBe('P');
    expect((summary?.textContent ?? '').length).toBeLessThan(200);
  });

  /** Skimmable with a reader's heading key, which is how somebody who came here for one
   * of the three questions finds theirs without reading the other two. */
  it('is broken into headings under its title', () => {
    expect(dialog.querySelector('h1')?.id).toBe('help-title');
    expect(dialog.querySelectorAll('h2').length).toBeGreaterThanOrEqual(2);
  });

  /** The vocabulary test, and it is a domain requirement rather than style: the sentence
   * that sends a user here was rewritten because the product's own author could not
   * understand it, and a topic that explains it in the same words would undo that. */
  it('explains without using the words a listener does not have', () => {
    const text = (dialog.textContent ?? '').toLowerCase();

    for (const jargon of ['osc', 'marker', 'integration', 'unintegrated', 'verdict']) {
      expect(text, `"${jargon}" is this project's word, not a user's`).not.toContain(
        jargon,
      );
    }
  });
});
