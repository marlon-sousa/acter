// Role: adapter (DOM) — the host-key dialog: the security decision in SSH, put to a person
// who cannot see a wall of warning text.
//
// **This is the moment a terminal traditionally prints a paragraph nobody reads** (spec B9,
// decision 3). For this audience it has to be a real modal dialog with a real accessible
// name, saying in order: which host, that its key is unknown — or *changed*, which is a
// different and more serious sentence — the fingerprint in a form that can be read character
// by character, and what the choices mean.
//
// **The default is refusal, and it is the default in three separate ways**, because one is
// not enough for a decision this consequential: the refusing button holds initial focus, it
// is the dialog's own cancel action, and closing the dialog by any means at all answers
// "give up". Nothing here can produce a "trust" answer except pressing the button that says
// so.

import { keepTabInside } from './dialog_tab';
import type { ConnectAnswer, ConnectQuestion } from '../protocol';

/** What the dialog is called, which is the first thing a reader announces. */
const UNKNOWN_TITLE = 'Unknown server';
const CHANGED_TITLE = 'Warning: this server has changed';

/**
 * What a changed key means, said plainly and without softening it.
 *
 * **A changed key is either a rebuilt server or somebody sitting between the user and it**,
 * and there is no way for Acter to tell which. Saying so is the whole point: a cheerful
 * "continue?" here would spend the one alarming sentence this product has on nothing.
 */
const CHANGED_WARNING =
  'The server at this address is not the one Acter connected to before. Either it was rebuilt, or something is pretending to be it. If you were not expecting this, do not connect.';

const UNKNOWN_EXPLANATION =
  'Acter has never connected to this server before, so it has nothing to compare its identity against. Check the fingerprint below against one you trust before connecting.';

export class HostKeyDialog {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly title: HTMLElement,
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
   * **Every way out that is not the trust button resolves to giving up**: Escape, the
   * refusing button, and the dialog being closed by anything else. The promise settles on
   * `close`, which is the one event all of them go through.
   */
  ask(question: Extract<ConnectQuestion, { question: 'HostKey' }>): Promise<ConnectAnswer> {
    const document = this.body.ownerDocument;
    const changed = question.recorded !== null;
    this.title.textContent = changed ? CHANGED_TITLE : UNKNOWN_TITLE;
    this.dialog.setAttribute(
      'aria-label',
      changed ? CHANGED_TITLE : UNKNOWN_TITLE,
    );

    // **What a reader says as the dialog opens**, because a dialog that announces only its
    // own name and its focused button leaves a listener to go looking for the question.
    this.summary.textContent = [
      `${question.host}, port ${question.port}.`,
      changed ? CHANGED_WARNING : UNKNOWN_EXPLANATION,
      // Something true that is not the answer: a `known_hosts` file that could not be read,
      // so the user knows this may be being asked about a host they already trust. It is
      // said with the rest rather than left as one more thing to go and find.
      question.aside,
    ]
      .filter((part) => part !== null && part !== '')
      .join(' ');

    const said = document.createElement('div');
    if (changed) {
      said.append(
        fingerprint(
          document,
          'host-key-recorded',
          'Fingerprint Acter recorded before',
          question.recorded ?? '',
        ),
      );
    }
    said.append(
      fingerprint(
        document,
        'host-key-offered',
        'Fingerprint this server is offering now',
        question.fingerprint,
      ),
    );
    this.body.replaceChildren(said);

    return new Promise<ConnectAnswer>((resolve) => {
      // **One place decides the answer**, and its default is refusal: whatever closed the
      // dialog, only the trust button will have set `returnValue`.
      const settle = (): void => {
        this.dialog.removeEventListener('close', settle);
        resolve(
          this.dialog.returnValue === 'trust'
            ? { answer: 'Trust' }
            : { answer: 'GiveUp' },
        );
      };
      this.dialog.addEventListener('close', settle);
      this.dialog.returnValue = '';
      this.dialog.showModal();
      // **Focus starts on the fingerprint, which is the thing they are here to read**
      // (asked for by the user, 2026-08-26). It is safe to land here precisely because
      // Enter does nothing: the only way out is a button somebody chose to go to.
      this.offered()?.focus();
    });
  }

  private offered(): HTMLElement | null {
    return this.dialog.querySelector<HTMLElement>('#host-key-offered');
  }

  /**
   * **This dialog has no default action, and that is the point of it.**
   *
   * Asked for by the user on 2026-08-26: "we need a planned user action." Everywhere else
   * in this product Enter is the obliging key — it submits the connect form, it activates
   * the focused control. Here it must not be, because the two outcomes are "trust this
   * server" and "do not", and neither should be reachable by the key somebody presses
   * without thinking. A form's implicit submission would otherwise pick a button for them.
   *
   * A button that *has* focus still answers Enter, because going to it is itself the
   * deliberate act — which is the difference between choosing and defaulting.
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

/**
 * A fingerprint, as a **read-only edit field**.
 *
 * **Reported by the user on 2026-08-26, and it was a real defect rather than a preference.**
 * It used to be a focusable `<code>`, on the reasoning that a fingerprint has to be read
 * character by character and therefore needs somewhere to put a cursor. That reasoning was
 * half right and the conclusion was wrong: this dialog is inside `role="application"`, where
 * the arrows do **not** read prose, so the only way to walk a paragraph is NVDA's review
 * cursor — which is outside the vocabulary an ordinary user is assumed to have. The thing it
 * most needed to be comparable was the thing it was not.
 *
 * A read-only text box is arrowable with the plain arrow keys in every mode, which is what
 * comparing forty-three characters of mixed-case base64 against a printed one actually takes.
 * `readonly` rather than `disabled`, because a disabled control is skipped by focus entirely.
 */
function fingerprint(document: Document, id: string, label: string, value: string): HTMLElement {
  const group = document.createElement('p');
  const name = document.createElement('label');
  name.htmlFor = id;
  name.textContent = label;
  const said = document.createElement('input');
  said.id = id;
  said.type = 'text';
  said.value = value;
  // **Editable, with every edit refused** — and that is the fix rather than a compromise.
  //
  // Measured with NVDA 2026.1.1 on 2026-08-26, after the user reported twice that the
  // arrows did nothing here. With `readonly` set, the field has focus and reports its value
  // through the accessibility API — `get_focus_info` returned role EDITABLETEXT, states
  // READONLY FOCUSABLE FOCUSED, and the full fingerprint as its value — and yet **every**
  // caret key answered "blank": Right, Left, Home and End alike. The same dialog's editable
  // Host field, arrowed in the same session, read "1", "2", "2". So a read-only input in
  // this webview exposes a value with no caret-navigable text behind it, and a fingerprint
  // nobody can walk is a fingerprint nobody can compare.
  //
  // Refusing the edit at `beforeinput` keeps the caret a real caret while making the value
  // unchangeable — including by paste, which is one of the input types this cancels.
  said.addEventListener('beforeinput', (event) => event.preventDefault());
  // Belt and braces, for any path that reaches the value without a cancellable event.
  said.addEventListener('input', () => {
    said.value = value;
  });
  // Nothing here is a thing to fill in, and a browser offering to complete a fingerprint
  // would be offering the one value that must not come from anywhere but the server.
  said.autocomplete = 'off';
  said.spellcheck = false;
  group.append(name, said);
  return group;
}
