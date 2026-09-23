// @vitest-environment jsdom
// Role: test — the host-key dialog, which is the security decision in SSH.

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

    expect(summary).toContain('acter-ssh');
    expect(summary).toContain('2222');
    expect(summary).toContain('never connected to this server before');
    expect(offered.value).toBe(UNKNOWN.fingerprint);

    dialog.close('refuse');
    await answered;
  });

  it('puts the fingerprint somewhere the arrow keys can walk', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);
    const shown = document.getElementById('host-key-offered') as HTMLInputElement;

    expect(shown.tagName).toBe('INPUT');
    expect(shown.value).toBe(UNKNOWN.fingerprint);
    expect(shown.readOnly).toBe(false);
    expect(
      document.querySelector('label[for="host-key-offered"]')?.textContent,
    ).toContain('offering now');

    dialog.close('refuse');
    await answered;
  });

  it('refuses every attempt to change the fingerprint', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);
    const shown = document.getElementById('host-key-offered') as HTMLInputElement;

    const edit = new Event('beforeinput', { cancelable: true, bubbles: true });
    shown.dispatchEvent(edit);
    expect(edit.defaultPrevented).toBe(true);

    shown.value = 'SHA256:something-else-entirely';
    shown.dispatchEvent(new Event('input', { bubbles: true }));
    expect(shown.value).toBe(UNKNOWN.fingerprint);

    dialog.close('refuse');
    await answered;
  });

  it('has no paragraph in the tab order', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);

    expect(document.querySelectorAll('p[tabindex="0"]').length).toBe(0);

    dialog.close('refuse');
    await answered;
  });

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

  it.each(['refuse', '', 'anything else'])(
    'gives up when the dialog closes with %o',
    async (value) => {
      const { dialog, ask } = build();

      const answered = ask.ask(UNKNOWN);
      dialog.close(value);

      await expect(answered).resolves.toEqual({ answer: 'GiveUp' });
    },
  );

  it('opens with focus on the fingerprint', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);

    expect(document.activeElement?.id).toBe('host-key-offered');

    dialog.close('refuse');
    await answered;
  });

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

  it('passes on an aside about a file it could not read', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask({
      ...UNKNOWN,
      aside: 'Acter could not read your own OpenSSH known hosts file.',
    });

    expect(document.getElementById('host-key-summary')?.textContent).toContain(
      'could not read',
    );

    dialog.close('refuse');
    await answered;
  });
});
