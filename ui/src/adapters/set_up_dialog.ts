// Role: adapter (DOM) — the dialog that discloses the one command Acter would run inside a
// session once that session is established.
//
// **The checkbox authorises and this discloses, and neither is optional** (spec B9.5,
// decision 9). Being on by default is what makes an ordinary user hear a heading for each
// command and be told when one fails without knowing the words this project uses — A13's
// whole subject — and it keeps the rule that nothing in this product is a gate. What stops
// "on by default" from being a surprise is this dialog.
//
// **The one dialog on this seam that is not a warning, and its default is to continue.** A
// host key and an unverified file are security decisions where the safe answer is the one
// that does nothing; this is Acter offering to tell a listener more about their own session,
// and the thing it defends against is surprise rather than harm. So Continue holds initial
// focus after the command, where the host-key dialog puts the refusing button — and
// cancelling is still one keystroke away, because refusing has to be as reachable as
// accepting.
//
// **Every sentence here is the backend's.** What was detected, what the person gets, and what
// refusing costs are composed in the domain, in one place, and rendered here (spec B9.5,
// decision 9) — the rule the unverified dialog established. What this file owns is the shape:
// paragraphs a listener can arrow, and the command in a field they can walk character by
// character.
//
// **Cancelling refuses this session only**, and says so through the connection sentence. The
// Connect dialog's checkbox is what refuses durably.

import { keepTabInside } from './dialog_tab';
import { readableField } from './readable_field';
import type { ConnectAnswer, ConnectQuestion } from '../protocol';

/** The box holding the command, labelled as what a listener is being asked to look at. */
const COMMAND = 'set-up-command';

/** The value the continuing button sets, and the only thing that produces a "set up" answer. */
const SET_UP = 'set-up';

/** What the command's box is called: a whole phrase, because it is read aloud as a label. */
const COMMAND_LABEL = 'This is the command Acter will run for you';

export class SetUpDialog {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly summary: HTMLElement,
    private readonly body: HTMLElement,
    private readonly remember: HTMLInputElement,
  ) {
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
    this.dialog.addEventListener('keydown', (event) => this.noDefaultAction(event));
  }

  /**
   * Puts the question, and resolves with what was decided.
   *
   * **Every way out that is not the continuing button resolves to skipping it**: Escape, the
   * cancelling button, and the dialog being closed by anything else. The promise settles on
   * `close`, which is the one event all of them go through.
   */
  ask(
    question: Extract<ConnectQuestion, { question: 'SetUpSession' }>,
  ): Promise<ConnectAnswer> {
    const document = this.body.ownerDocument;
    // **What a reader says as the dialog opens.** Two sentences: what was detected, and what
    // the person gets for saying yes — which for a shell that reaches only the prompt
    // boundaries also says what they will not get.
    this.summary.textContent = `${question.detected} ${question.offer}`;

    const said = document.createElement('div');
    said.append(
      // The command verbatim, in a box the plain arrow keys can walk. It is the disclosure
      // the whole dialog is: the same treatment a host-key fingerprint and a program path
      // get, and for the same reason — a value that has to be checked by hand.
      readableField(document, COMMAND, COMMAND_LABEL, question.command),
    );
    // What refusing costs, last, so it is the sentence a listener arrives at before the
    // controls. It is A13's shipped sentence with what still works in front of it, and it is
    // the backend's words rather than this file's.
    //
    // **It is focusable, and that is a measured requirement rather than a flourish.** This
    // dialog is inside `role="application"`, where the arrows belong to the widget and prose
    // cannot be arrowed at all — the cost the Connect dialog's own panel records and pays the
    // same way. Found in the NVDA pass for this entry: the two sentences above are announced
    // as the dialog opens, through its description, and this one was reachable by nothing.
    // A listener could read the command Acter was about to run and never hear what saying no
    // would cost them, which is the one sentence the dialog is built around.
    const refusal = document.createElement('p');
    refusal.tabIndex = 0;
    refusal.textContent = question.refusal;
    said.append(refusal);
    this.body.replaceChildren(said);

    // A dialog asked twice in one window starts from an unticked box each time: "do not show
    // this again" is a decision about the dialog in front of the user now.
    this.remember.checked = false;

    return new Promise<ConnectAnswer>((resolve) => {
      // **One place decides the answer**, and only the continuing button will have set
      // `returnValue` — so a dialog closed by any other means skips the setup.
      const settle = (): void => {
        this.dialog.removeEventListener('close', settle);
        resolve(
          this.dialog.returnValue === SET_UP
            ? { answer: 'SetUpSession', remember: this.remember.checked }
            : { answer: 'GiveUp' },
        );
      };
      this.dialog.addEventListener('close', settle);
      this.dialog.returnValue = '';
      this.dialog.showModal();
      // **Focus starts on the command, which is the thing they are here to read** — the
      // placement the host-key dialog gives the fingerprint and the unverified dialog gives
      // the path. Safe for the same reason: Enter does nothing here, so the only way out is a
      // button somebody chose to go to.
      this.dialog.querySelector<HTMLElement>(`#${COMMAND}`)?.focus();
    });
  }

  /**
   * **No default action, for the reason the other two dialogs have none.**
   *
   * The rule the host-key dialog established on 2026-08-26 — "we need a planned user action"
   * — applies here even though neither outcome is dangerous: what Enter must not do is decide
   * on somebody's behalf whether a command runs in their session. A button that *has* focus
   * still answers Enter, because going to it is itself the deliberate act.
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
