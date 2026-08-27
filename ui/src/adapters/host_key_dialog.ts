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
    this.summary.textContent = `${question.host}, port ${question.port}. ${
      changed ? CHANGED_WARNING : UNKNOWN_EXPLANATION
    }`;

    const said = document.createElement('div');
    if (changed) {
      said.append(
        fingerprint(document, 'Fingerprint Acter recorded before', question.recorded ?? ''),
      );
    }
    said.append(
      fingerprint(document, 'Fingerprint this server is offering now', question.fingerprint),
    );
    // Something true that is not the answer: a `known_hosts` file that could not be read, so
    // the user knows this may be being asked about a host they already trust.
    if (question.aside !== null) {
      said.append(paragraph(document, question.aside));
    }
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
      // Focus starts on the refusing button, so the key a listener presses without thinking
      // — Enter, on whatever the dialog handed them — does the safe thing.
      this.refuse()?.focus();
    });
  }

  private refuse(): HTMLElement | null {
    return this.dialog.querySelector<HTMLElement>('#host-key-refuse');
  }
}

function paragraph(document: Document, text: string): HTMLElement {
  const said = document.createElement('p');
  // Prose inside an application region cannot be arrowed, so without a tab stop the thing
  // a user most needs to read here would be unreachable (spec A8's instructions panel, same
  // reasoning).
  said.tabIndex = 0;
  said.textContent = text;
  return said;
}

/**
 * A fingerprint, labelled and reachable.
 *
 * **Its own focusable element, because a fingerprint is read character by character.** It is
 * forty-odd characters of mixed-case base64 with no words in it, and a listener compares it
 * against something a provider printed — which means moving through it one character at a
 * time with the reader's own review commands, and that needs somewhere to put the cursor.
 */
function fingerprint(document: Document, label: string, value: string): HTMLElement {
  const group = document.createElement('p');
  const name = document.createElement('span');
  name.textContent = `${label}: `;
  const said = document.createElement('code');
  said.tabIndex = 0;
  said.setAttribute('aria-label', `${label}, ${value}`);
  said.textContent = value;
  group.append(name, said);
  return group;
}
