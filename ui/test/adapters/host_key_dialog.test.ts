// @vitest-environment jsdom
// Role: test — the host-key dialog, which is the security decision in SSH.
//
// What is asserted here is mostly about **refusal being the default**, because that is the
// one property a mistake in this file would quietly take away.

import { beforeEach, describe, expect, it } from 'vitest';

import { HostKeyDialog } from '../../src/adapters/host_key_dialog';
import type { ConnectQuestion } from '../../src/protocol';

type HostKey = Extract<ConnectQuestion, { question: 'HostKey' }>;

const UNKNOWN: HostKey = {
  question: 'HostKey',
  host: 'acter-ssh',
  port: 2222,
  fingerprint: 'SHA256:IzJE9oHP7rabiNsCSTceP2l1jW8/4WESW2jkk+JFiOU',
  recorded: null,
  aside: null,
};

const CHANGED: HostKey = { ...UNKNOWN, recorded: 'SHA256:somethingElse' };

function build(): { dialog: HTMLDialogElement; ask: HostKeyDialog } {
  document.body.innerHTML = `
    <dialog id="host-key-dialog">
      <h1 id="host-key-title"></h1>
      <p id="host-key-summary"></p>
      <div id="host-key-body"></div>
      <form method="dialog">
        <button id="host-key-refuse" type="submit" value="refuse">Do not connect</button>
        <button id="host-key-trust" type="submit" value="trust">Connect</button>
      </form>
    </dialog>`;
  const dialog = document.getElementById('host-key-dialog') as HTMLDialogElement;
  // jsdom implements `<dialog>` only partially depending on version; these keep the suite
  // about Acter's behaviour rather than about jsdom's coverage of the element. `close`
  // carries the value the way the platform does, because the returned value is exactly what
  // this adapter reads the decision from.
  dialog.showModal ??= function showModal(this: HTMLDialogElement) {
    this.open = true;
  };
  dialog.close ??= function close(this: HTMLDialogElement, value?: string) {
    this.open = false;
    if (value !== undefined) {
      this.returnValue = value;
    }
    this.dispatchEvent(new Event('close'));
  };
  return {
    dialog,
    ask: new HostKeyDialog(
      dialog,
      document.getElementById('host-key-title') as HTMLElement,
      document.getElementById('host-key-summary') as HTMLElement,
      document.getElementById('host-key-body') as HTMLElement,
    ),
  };
}

describe('HostKeyDialog', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  it('names the host and shows the fingerprint that was offered', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);
    const summary = document.getElementById('host-key-summary')?.textContent ?? '';
    const offered = document.getElementById('host-key-offered') as HTMLInputElement;

    // **Spoken as the dialog opens**, via aria-describedby: which host, and why it is
    // asking. Found by driving NVDA on 2026-08-26, where a dialog that carried this only
    // in its body announced its own name and nothing else.
    expect(summary).toContain('acter-ssh');
    expect(summary).toContain('2222');
    expect(summary).toContain('never connected to this server before');
    expect(offered.value).toBe(UNKNOWN.fingerprint);

    dialog.close('refuse');
    await answered;
  });

  // **A read-only edit field, not prose** — reported by the user on 2026-08-26 and it was a
  // real defect. This dialog is inside `role="application"`, where the arrows do not read a
  // paragraph, so the only way to walk one is a review cursor: outside the vocabulary an
  // ordinary user is assumed to have. A text box arrows in every mode, which is what
  // comparing forty-three characters against a printed value actually takes.
  it('puts the fingerprint somewhere the arrow keys can walk', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);
    const shown = document.getElementById('host-key-offered') as HTMLInputElement;

    expect(shown.tagName).toBe('INPUT');
    expect(shown.value).toBe(UNKNOWN.fingerprint);
    // **Not `readonly`, and that is deliberate** — measured with NVDA on 2026-08-26: a
    // read-only input in this webview reports its value and answers "blank" to every caret
    // key, so the one thing a fingerprint must be, walkable, was the thing it was not.
    expect(shown.readOnly).toBe(false);
    // Labelled by a real label, now that it is a real control.
    expect(
      document.querySelector('label[for="host-key-offered"]')?.textContent,
    ).toContain('offering now');

    dialog.close('refuse');
    await answered;
  });

  // Editable so the caret is real; every edit refused so the value cannot change.
  it('refuses every attempt to change the fingerprint', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);
    const shown = document.getElementById('host-key-offered') as HTMLInputElement;

    const edit = new Event('beforeinput', { cancelable: true, bubbles: true });
    shown.dispatchEvent(edit);
    expect(edit.defaultPrevented).toBe(true);

    // And anything that got past a cancellable event is put back.
    shown.value = 'SHA256:something-else-entirely';
    shown.dispatchEvent(new Event('input', { bubbles: true }));
    expect(shown.value).toBe(UNKNOWN.fingerprint);

    dialog.close('refuse');
    await answered;
  });

  // Nothing in this dialog is a paragraph to tab to: a paragraph is not a control, and one
  // in the tab order is a stop that answers nothing (reported 2026-08-26).
  it('has no paragraph in the tab order', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);

    expect(document.querySelectorAll('p[tabindex="0"]').length).toBe(0);

    dialog.close('refuse');
    await answered;
  });

  // **A changed key is a different sentence, not the same one with a different value.**
  it('says something different, and more serious, about a changed key', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(CHANGED);
    const title = document.getElementById('host-key-title')?.textContent ?? '';
    const summary = document.getElementById('host-key-summary')?.textContent ?? '';
    const recorded = document.getElementById('host-key-recorded') as HTMLInputElement;
    const offered = document.getElementById('host-key-offered') as HTMLInputElement;

    expect(title.toLowerCase()).toContain('changed');
    expect(summary).toContain('pretending to be it');
    expect(recorded.value).toBe(CHANGED.recorded);
    expect(offered.value).toBe(CHANGED.fingerprint);

    dialog.close('refuse');
    await answered;
  });

  it('trusts the server only when the trusting button was pressed', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);
    dialog.close('trust');

    await expect(answered).resolves.toEqual({ answer: 'Trust' });
  });

  // **The heart of decision 3.** Escape, the refusing button, and anything else that closes
  // this dialog all mean the same thing, and it is not "connect".
  it.each(['refuse', '', 'anything else'])(
    'gives up when the dialog closes with %o',
    async (value) => {
      const { dialog, ask } = build();

      const answered = ask.ask(UNKNOWN);
      dialog.close(value);

      await expect(answered).resolves.toEqual({ answer: 'GiveUp' });
    },
  );

  // **Focus starts on the thing they are here to read** (asked for 2026-08-26). It is safe
  // to land on the fingerprint rather than on the refusing button precisely because this
  // dialog has no default action.
  it('opens with focus on the fingerprint', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);

    expect(document.activeElement?.id).toBe('host-key-offered');

    dialog.close('refuse');
    await answered;
  });

  /**
   * **No default action.** "We need a planned user action" — the two outcomes here are
   * trust and refuse, and neither may be reachable by the key somebody presses without
   * thinking. A form's implicit submission would otherwise choose one for them.
   */
  it('does nothing when Enter is pressed anywhere but a button', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);
    const field = document.getElementById('host-key-offered') as HTMLInputElement;
    const enter = new KeyboardEvent('keydown', {
      key: 'Enter',
      bubbles: true,
      cancelable: true,
    });
    field.dispatchEvent(enter);

    expect(enter.defaultPrevented).toBe(true);
    expect(dialog.open).toBe(true);

    dialog.close('refuse');
    await answered;
  });

  /** A button somebody went to still answers Enter: going there was the deliberate act. */
  it('still lets a focused button be pressed with Enter', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);
    const button = document.getElementById('host-key-trust') as HTMLButtonElement;
    const enter = new KeyboardEvent('keydown', {
      key: 'Enter',
      bubbles: true,
      cancelable: true,
    });
    button.dispatchEvent(enter);

    expect(enter.defaultPrevented).toBe(false);

    dialog.close('refuse');
    await answered;
  });

  // A `known_hosts` file that could not be read means this may be being asked about a host
  // the user already trusts, and they are told so rather than left to wonder.
  it('passes on an aside about a file it could not read', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask({
      ...UNKNOWN,
      aside: 'Acter could not read your own OpenSSH known hosts file.',
    });

    // Said with the rest as the dialog opens, rather than left as one more thing to find.
    expect(document.getElementById('host-key-summary')?.textContent).toContain(
      'could not read',
    );

    dialog.close('refuse');
    await answered;
  });
});
