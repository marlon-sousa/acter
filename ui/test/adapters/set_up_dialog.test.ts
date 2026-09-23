// @vitest-environment jsdom
// Role: test — the dialog that discloses the one command Acter would run inside a session.

import { beforeEach, describe, expect, it } from 'vitest';

import { SetUpDialog } from '../../src/adapters/set_up_dialog';
import type { ConnectQuestion } from '../../src/protocol';

type SetUpSession = Extract<ConnectQuestion, { question: 'SetUpSession' }>;

const BASH: SetUpSession = {
  question: 'SetUpSession',
  shell: 'bash',
  detected: 'Acter has detected that this session runs bash.',
  offer:
    'Acter can set it up so it tells you more about what you run. You are told when a command fails, and Acter can tell when each command has finished.',
  command: "printf 'mark'; PROMPT_COMMAND=__acter_prompt",
  refusal:
    'If you skip this, the session still works. You will hear what commands print here, but not whether they worked.',
};

const SH: SetUpSession = {
  ...BASH,
  shell: 'sh',
  detected: 'Acter has detected that this session runs sh.',
  offer:
    'Acter can set it up so it tells you more about what you run. Acter can tell when each command has finished. It cannot yet tell you when a command fails in this shell.',
};

function build(): {
  dialog: HTMLDialogElement;
  remember: HTMLInputElement;
  ask: SetUpDialog;
} {
  document.body.innerHTML = `
    <dialog id="set-up-dialog">
      <h1>Set this session up</h1>
      <p id="set-up-summary"></p>
      <div id="set-up-body"></div>
      <p>
        <input id="set-up-remember" type="checkbox" />
        <label for="set-up-remember">Do not show this dialog again</label>
      </p>
      <form method="dialog">
        <button id="set-up-continue" type="submit" value="set-up">Run command</button>
        <button id="set-up-cancel" type="submit" value="cancel">Skip</button>
      </form>
    </dialog>`;
  const dialog = document.getElementById('set-up-dialog') as HTMLDialogElement;
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
  const remember = document.getElementById('set-up-remember') as HTMLInputElement;
  return {
    dialog,
    remember,
    ask: new SetUpDialog(
      dialog,
      document.getElementById('set-up-summary') as HTMLElement,
      document.getElementById('set-up-body') as HTMLElement,
      remember,
    ),
  };
}

describe('SetUpDialog', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  it('says what was detected and what the person gets, as the dialog opens', () => {
    const { ask } = build();

    void ask.ask(BASH);

    const summary = document.getElementById('set-up-summary')?.textContent ?? '';
    expect(summary).toContain('this session runs bash');
    expect(summary).toContain('when each command has finished');
    expect(summary).toContain('told when a command fails');
  });

  it('says what a shell cannot do when the backend says it cannot', () => {
    const { ask } = build();

    void ask.ask(SH);

    const summary = document.getElementById('set-up-summary')?.textContent ?? '';
    expect(summary).toContain('cannot yet tell you when a command fails');
  });

  it('puts the command in a labelled box that can be walked and cannot be changed', () => {
    const { ask } = build();

    void ask.ask(BASH);

    const field = document.getElementById('set-up-command') as HTMLInputElement;
    expect(field).not.toBeNull();
    expect(field.value).toBe(BASH.command);
    const label = document.querySelector(`label[for="set-up-command"]`);
    expect(label?.textContent).toContain('command Acter will run');
    field.value = 'something else';
    field.dispatchEvent(new Event('input'));
    expect(field.value).toBe(BASH.command);
  });

  it('puts focus on the command rather than on a button', () => {
    const { ask } = build();

    void ask.ask(BASH);

    expect(document.activeElement?.id).toBe('set-up-command');
  });

  it('says what refusing costs, last, as the dialog opens', () => {
    const { ask } = build();

    void ask.ask(BASH);

    const summary = document.getElementById('set-up-summary')?.textContent ?? '';
    expect(summary).toContain(
      'You will hear what commands print here, but not whether they worked.',
    );
    expect(summary.trimEnd().endsWith('whether they worked.')).toBe(true);
  });

  it('puts nothing in the tab order that is not a control', () => {
    const { dialog, ask } = build();

    void ask.ask(BASH);

    const stops = Array.from(
      dialog.querySelectorAll<HTMLElement>('[tabindex]:not([tabindex="-1"])'),
    );
    expect(stops).toEqual([]);
  });

  it('answers that the session may be set up when Run command is pressed', async () => {
    const { dialog, ask } = build();

    const answering = ask.ask(BASH);
    dialog.close('set-up');

    await expect(answering).resolves.toEqual({
      answer: 'SetUpSession',
      remember: false,
    });
  });

  it('carries the do-not-ask-again box with the acceptance', async () => {
    const { dialog, remember, ask } = build();

    const answering = ask.ask(BASH);
    remember.checked = true;
    dialog.close('set-up');

    await expect(answering).resolves.toEqual({
      answer: 'SetUpSession',
      remember: true,
    });
  });

  it('gives up on every way out that is not Run command', async () => {
    for (const closedWith of ['cancel', '', 'set-up-typo']) {
      const { dialog, ask } = build();

      const answering = ask.ask(BASH);
      dialog.close(closedWith);

      await expect(answering).resolves.toEqual({ answer: 'GiveUp' });
    }
  });

  it('does not carry the do-not-ask-again box from one shell to the next', async () => {
    const { dialog, remember, ask } = build();
    const first = ask.ask(BASH);
    remember.checked = true;
    dialog.close('set-up');
    await first;

    const second = ask.ask(SH);
    expect(remember.checked).toBe(false);
    dialog.close('set-up');

    await expect(second).resolves.toEqual({
      answer: 'SetUpSession',
      remember: false,
    });
  });

  it('does nothing when Enter is pressed away from a button', () => {
    const { ask } = build();
    void ask.ask(BASH);

    const field = document.getElementById('set-up-command') as HTMLInputElement;
    const enter = new KeyboardEvent('keydown', {
      key: 'Enter',
      bubbles: true,
      cancelable: true,
    });
    field.dispatchEvent(enter);

    expect(enter.defaultPrevented).toBe(true);
  });

  it('names its buttons after what they do', () => {
    const { ask } = build();
    void ask.ask(BASH);

    expect(document.getElementById('set-up-continue')?.textContent).toBe('Run command');
    expect(document.getElementById('set-up-cancel')?.textContent).toBe('Skip');
  });

  it('lets a focused button answer Enter', () => {
    const { ask } = build();
    void ask.ask(BASH);

    const button = document.getElementById('set-up-continue') as HTMLButtonElement;
    const enter = new KeyboardEvent('keydown', {
      key: 'Enter',
      bubbles: true,
      cancelable: true,
    });
    button.dispatchEvent(enter);

    expect(enter.defaultPrevented).toBe(false);
  });
});
