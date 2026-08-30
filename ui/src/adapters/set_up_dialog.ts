// Role: adapter (DOM) — the dialog that discloses the one command Acter would run inside a
// session once that session is established.
//
// **The checkbox authorises and this discloses, and neither is optional** (spec B9.5,
// decision 9). Being on by default is what makes an ordinary user be told when a command
// failed without knowing the words this project uses — A13's whole subject — and it keeps the
// rule that nothing in this product is a gate. (It is *not* what gives them a heading for each
// command: a session has those either way, which the user caught this sentence claiming
// otherwise on 2026-08-30.) What stops
// "on by default" from being a surprise is this dialog.
//
// **The one dialog on this seam that is not a warning, and its default is to continue.** A
// host key and an unverified file are security decisions where the safe answer is the one
// that does nothing; this is Acter offering to tell a listener more about their own session,
// and the thing it defends against is surprise rather than harm. So the running button holds
// initial focus after the command, where the host-key dialog puts the refusing button — and
// skipping is still one keystroke away, because refusing has to be as reachable as accepting.
//
// **The two buttons say what they do**: "Run command" and "Skip", asked for by the user on
// 2026-08-30 in place of "Continue" and "Cancel". Those are the words for a dialog whose
// question is whether to go on; this one asks whether a command runs in your shell, and a
// listener who arrives on a button should hear the answer to that.
//
// **Every sentence here is the backend's.** What was detected, what the person gets, and what
// refusing costs are composed in the domain, in one place, and rendered here (spec B9.5,
// decision 9) — the rule the unverified dialog established. What this file owns is the shape:
// the three sentences as the dialog's own description, and the command in a field a listener
// can walk character by character.
//
// **All three sentences are the description, and none of them is a tab stop** — reported by
// the user on 2026-08-30, who met the refusal sentence as a focusable paragraph while tabbing
// this dialog. A paragraph is not a control, and putting one in the tab order to make it
// reachable teaches a listener that Tab lands on things that do nothing. The reachability it
// was buying is real — inside `role="application"` prose cannot be arrowed — and it is bought
// instead by the one mechanism that speaks prose in a dialog without any control at all: the
// description a reader reads out as the dialog opens. So the dialog says what was detected,
// what saying yes gives, and what saying no costs, in one utterance, and the only things Tab
// finds are the command, the box and the two buttons.
//
// **Skipping refuses this session only**, and says so through the connection sentence. The
// Connect dialog's checkbox is what refuses durably.

import { keepTabInside } from './dialog_tab';
import { readableField } from './readable_field';
import type { ConnectAnswer, ConnectQuestion } from '../protocol';

/** The box holding the command, labelled as what a listener is being asked to look at. */
const COMMAND = 'set-up-command';

/** The value the running button sets, and the only thing that produces a "set up" answer. */
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
   * **Every way out that is not "Run command" resolves to skipping it**: Escape, the Skip
   * button, and the dialog being closed by anything else. The promise settles on `close`,
   * which is the one event all of them go through.
   */
  ask(
    question: Extract<ConnectQuestion, { question: 'SetUpSession' }>,
  ): Promise<ConnectAnswer> {
    const document = this.body.ownerDocument;
    // **What a reader says as the dialog opens.** Three sentences: what was detected, what the
    // person gets for saying yes — which for a shell that reaches only the prompt boundaries
    // also says what they will not get — and what saying no costs. The last of them is here
    // rather than in the body because this is the one place prose is spoken inside an
    // application region without being a tab stop.
    this.summary.textContent = `${question.detected} ${question.offer} ${question.refusal}`;

    const said = document.createElement('div');
    said.append(
      // The command verbatim, in a box the plain arrow keys can walk. It is the disclosure
      // the whole dialog is: the same treatment a host-key fingerprint and a program path
      // get, and for the same reason — a value that has to be checked by hand.
      readableField(document, COMMAND, COMMAND_LABEL, question.command),
    );
    this.body.replaceChildren(said);

    // A dialog asked twice in one window starts from an unticked box each time: "do not show
    // this again" is a decision about the dialog in front of the user now.
    this.remember.checked = false;

    return new Promise<ConnectAnswer>((resolve) => {
      // **One place decides the answer**, and only "Run command" will have set
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
