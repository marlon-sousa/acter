// @vitest-environment jsdom
// Role: test — the dialog that discloses the one command Acter would run inside a session.
//
// What is asserted here is that the disclosure is complete and readable, and that **nothing
// but pressing Continue sets a session up** — the property a mistake in this file would
// quietly take away, and the reason the checkbox on the Connect dialog is not enough on its
// own (spec B9.5, decision 9).

import { beforeEach, describe, expect, it } from 'vitest';

import { SetUpDialog } from '../../src/adapters/set_up_dialog';
import type { ConnectQuestion } from '../../src/protocol';

type SetUpSession = Extract<ConnectQuestion, { question: 'SetUpSession' }>;

/** What the backend composes for a shell whose setup reaches every boundary. */
const BASH: SetUpSession = {
  question: 'SetUpSession',
  shell: 'bash',
  detected: 'Acter has detected that this session runs bash.',
  offer:
    'Acter can set it up so it tells you more about what you run. You get a heading for each command, and you are told when a command fails.',
  command: "printf 'mark'; PROMPT_COMMAND=__acter_prompt",
  refusal:
    'If you cancel, the session still works. You will hear what commands print here, but not whether they worked.',
};

/** And for one that reaches the prompt boundaries and no further (decision 8). */
const SH: SetUpSession = {
  ...BASH,
  shell: 'sh',
  detected: 'Acter has detected that this session runs sh.',
  offer:
    'Acter can set it up so it tells you more about what you run. You get a heading for each command. Acter cannot yet tell you when a command fails in this shell.',
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
        <button id="set-up-continue" type="submit" value="set-up">Continue</button>
        <button id="set-up-cancel" type="submit" value="cancel">Cancel</button>
      </form>
    </dialog>`;
  const dialog = document.getElementById('set-up-dialog') as HTMLDialogElement;
  // jsdom implements `<dialog>` only partially depending on version; these keep the suite
  // about Acter's behaviour rather than about jsdom's coverage of the element.
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

  /**
   * **What a reader says as the dialog opens**, because a dialog that announces only its own
   * name and its focused control leaves a listener to go looking for the question.
   */
  it('says what was detected and what the person gets, as the dialog opens', () => {
    const { ask } = build();

    void ask.ask(BASH);

    const summary = document.getElementById('set-up-summary')?.textContent ?? '';
    expect(summary).toContain('this session runs bash');
    expect(summary).toContain('a heading for each command');
    expect(summary).toContain('told when a command fails');
  });

  /**
   * **The sentence has to be able to say "partly"** (spec B9.5, decision 8), and it is the
   * backend's sentence rather than one this file assembles — so a shell that cannot report a
   * failure says so here without this dialog knowing what a marker is.
   */
  it('says what a shell cannot do when the backend says it cannot', () => {
    const { ask } = build();

    void ask.ask(SH);

    const summary = document.getElementById('set-up-summary')?.textContent ?? '';
    expect(summary).toContain('cannot yet tell you when a command fails');
  });

  /**
   * **The disclosure the whole dialog is** (spec B9.5, decision 3): the command verbatim, in
   * a box the plain arrow keys can walk — the treatment a host-key fingerprint gets, because
   * a value nobody can read character by character is a value nobody can check.
   */
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

  /** And focus starts there, which is the thing they opened this to read. */
  it('puts focus on the command rather than on a button', () => {
    const { ask } = build();

    void ask.ask(BASH);

    expect(document.activeElement?.id).toBe('set-up-command');
  });

  /**
   * **What refusing costs, in the user's words** — A13's shipped sentence, which is the
   * register test rather than a placeholder.
   *
   * **It is the last sentence of the description, and nothing in the body** — reported by
   * the user on 2026-08-30, who met it as a focusable paragraph while tabbing this dialog.
   * A paragraph is not a control; the description is how prose is spoken inside an
   * application region without being a tab stop.
   */
  it('says what refusing costs, last, as the dialog opens', () => {
    const { ask } = build();

    void ask.ask(BASH);

    const summary = document.getElementById('set-up-summary')?.textContent ?? '';
    expect(summary).toContain(
      'You will hear what commands print here, but not whether they worked.',
    );
    expect(summary.trimEnd().endsWith('whether they worked.')).toBe(true);
  });

  /** And the only things Tab finds are the command, the box and the two buttons. */
  it('puts nothing in the tab order that is not a control', () => {
    const { dialog, ask } = build();

    void ask.ask(BASH);

    const stops = Array.from(
      dialog.querySelectorAll<HTMLElement>('[tabindex]:not([tabindex="-1"])'),
    );
    expect(stops).toEqual([]);
  });

  it('answers that the session may be set up when Continue is pressed', async () => {
    const { dialog, ask } = build();

    const answering = ask.ask(BASH);
    dialog.close('set-up');

    await expect(answering).resolves.toEqual({
      answer: 'SetUpSession',
      remember: false,
    });
  });

  /**
   * **"Do not show this dialog again" travels with the acceptance**, and is kept per shell by
   * the backend rather than by this dialog (spec B9.5, decision 10).
   */
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

  /**
   * **Nothing but the button that says so sets a session up.** Cancelling, Escape and every
   * other way of closing this refuse — and refuse *this session only*, which the connection
   * sentence then says out loud.
   */
  it('gives up on every way out that is not Continue', async () => {
    for (const closedWith of ['cancel', '', 'set-up-typo']) {
      const { dialog, ask } = build();

      const answering = ask.ask(BASH);
      dialog.close(closedWith);

      await expect(answering).resolves.toEqual({ answer: 'GiveUp' });
    }
  });

  /**
   * **A second dialog starts from an unticked box**, because "do not show this again" is a
   * decision about the dialog in front of the user now — not one that leaks from the last
   * shell they were asked about.
   */
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

  /**
   * **No default action**, for the reason the host-key dialog has none: what Enter must not
   * do is decide on somebody's behalf whether a command runs in their session.
   */
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

  /** A button that has focus still answers Enter: going to it is the deliberate act. */
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
