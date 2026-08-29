// Role: adapter (DOM) — the dialog that asks whether to start a program Windows would not
// vouch for.
//
// **It is a question and never a gate** (spec B5.7, decision 6). Everything this machine has
// stays in the connect list — a self-built pwsh, a corporate re-signed build, a damaged
// catalog database and an offline revocation check are all legitimate and all common, and
// hiding any of them would teach a user that Acter cannot see the shell they are looking
// straight at. What the check buys is narrower and still worth it: it defeats `PATH`-order
// hijacking, and it tells the user *before* the program runs rather than after.
//
// **The shape is the host-key dialog's, deliberately** (spec B9, decision 3): a real modal
// with a real accessible name, saying what was found, what it means, and what the choices
// are — and the value a listener has to check by hand in a box they can walk with the plain
// arrow keys. The two dialogs are separate files because they say different things about
// different subjects, and one dialog with a mode would be one dialog nobody could describe.
//
// **The name is fixed rather than switched.** Unlike the host key, where an unknown key and
// a changed one are two very different pieces of news, every verdict that reaches this dialog
// is the same situation — Acter could not confirm who made this file — and which of the
// several ways that happened is the sentence inside rather than the name outside.
//
// **The default is not to start, and it is the default in three separate ways**: the
// refusing button holds initial focus after the path, it is the dialog's own cancel action,
// and closing the dialog by any means at all answers "give up". Nothing here can produce a
// "start it" answer except pressing the button that says so.

import { keepTabInside } from './dialog_tab';
import { readableField } from './readable_field';
import type { ConnectAnswer, ConnectQuestion } from '../protocol';

/** What the two boxes hold, labelled as what a listener is being asked to look at. */
const PROGRAM = 'unverified-program';
const SIGNER = 'unverified-signer';

/** The value the starting button sets, and the only thing that produces a "start" answer. */
const START = 'start';

export class UnverifiedDialog {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly summary: HTMLElement,
    private readonly body: HTMLElement,
  ) {
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
    this.dialog.addEventListener('keydown', (event) => this.noDefaultAction(event));
  }

  /**
   * Puts the question, and resolves with what was decided.
   *
   * **Every way out that is not the starting button resolves to giving up**: Escape, the
   * refusing button, and the dialog being closed by anything else. The promise settles on
   * `close`, which is the one event all of them go through.
   */
  ask(
    question: Extract<ConnectQuestion, { question: 'Unverified' }>,
  ): Promise<ConnectAnswer> {
    const document = this.body.ownerDocument;
    // **What a reader says as the dialog opens**, because a dialog that announces only its
    // own name and its focused button leaves a listener to go looking for the question. The
    // sentence after the name is the backend's own: what was found and what to do next are
    // decided in the domain, in one place, and rendered here (spec B5.7, decision 6).
    this.summary.textContent = `${question.label}. ${question.said}`;

    const said = document.createElement('div');
    said.append(
      // **The full path, and it is the point of the dialog.** The thing this check defeats
      // is somebody putting a different file where a name used to point, so which directory
      // the file is in is the whole of what makes it recognisably wrong — and it has to be
      // walkable character by character to be compared at all.
      readableField(document, PROGRAM, 'File Acter would start', question.program),
    );
    if (question.signer !== null) {
      said.append(
        readableField(document, SIGNER, 'Signed by', question.signer),
      );
    }
    this.body.replaceChildren(said);

    return new Promise<ConnectAnswer>((resolve) => {
      // **One place decides the answer**, and its default is not to start: whatever closed
      // the dialog, only the starting button will have set `returnValue`.
      const settle = (): void => {
        this.dialog.removeEventListener('close', settle);
        resolve(
          this.dialog.returnValue === START
            ? { answer: 'StartAnyway' }
            : { answer: 'GiveUp' },
        );
      };
      this.dialog.addEventListener('close', settle);
      this.dialog.returnValue = '';
      this.dialog.showModal();
      // **Focus starts on the path, which is the thing they are here to read** — the same
      // placement the host-key dialog gives the fingerprint, and safe for the same reason:
      // Enter does nothing here, so the only way out is a button somebody chose to go to.
      this.dialog.querySelector<HTMLElement>(`#${PROGRAM}`)?.focus();
    });
  }

  /**
   * **This dialog has no default action, and that is the point of it.**
   *
   * The rule the host-key dialog established on 2026-08-26 — "we need a planned user
   * action" — applies here for the same reason: the two outcomes are "run this program" and
   * "do not", and neither should be reachable by the key somebody presses without thinking.
   *
   * A button that *has* focus still answers Enter, because going to it is itself the
   * deliberate act.
   */
  private noDefaultAction(event: KeyboardEvent): void {
    if (event.key !== 'Enter') {
      return;
    }
    if ((event.target as HTMLElement).closest('button') !== null) {
      return;
    }
    event.preventDefault();
  }
}
