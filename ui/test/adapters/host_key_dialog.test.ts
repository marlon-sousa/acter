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
    const said = document.getElementById('host-key-body')?.textContent ?? '';

    // **Spoken as the dialog opens**, via aria-describedby: which host, and why it is
    // asking. Found by driving NVDA on 2026-08-26, where a dialog that carried this only
    // in its body announced its own name and nothing else.
    expect(summary).toContain('acter-ssh');
    expect(summary).toContain('2222');
    expect(summary).toContain('never connected to this server before');
    expect(said).toContain(UNKNOWN.fingerprint);

    dialog.close('refuse');
    await answered;
  });

  // **The fingerprint is read character by character**, which needs somewhere to put the
  // reader's cursor — it is forty characters of mixed-case base64 with no words in it, and
  // a listener is comparing it against something a provider printed.
  it('makes the fingerprint reachable and labelled', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);
    const shown = document.querySelector('code');

    expect(shown?.tabIndex).toBe(0);
    expect(shown?.getAttribute('aria-label')).toContain(UNKNOWN.fingerprint);

    dialog.close('refuse');
    await answered;
  });

  // **A changed key is a different sentence, not the same one with a different value.**
  it('says something different, and more serious, about a changed key', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(CHANGED);
    const title = document.getElementById('host-key-title')?.textContent ?? '';
    const summary = document.getElementById('host-key-summary')?.textContent ?? '';
    const said = document.getElementById('host-key-body')?.textContent ?? '';

    expect(title.toLowerCase()).toContain('changed');
    expect(summary).toContain('pretending to be it');
    expect(said).toContain(CHANGED.recorded ?? '');
    expect(said).toContain(CHANGED.fingerprint);

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

  // The key a listener presses without thinking is Enter, on whatever the dialog handed
  // them — so what it hands them is the safe answer.
  it('opens with focus on the refusing button', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNKNOWN);

    expect(document.activeElement?.id).toBe('host-key-refuse');

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

    expect(document.getElementById('host-key-body')?.textContent).toContain(
      'could not read',
    );

    dialog.close('refuse');
    await answered;
  });
});
