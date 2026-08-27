// Role: adapter (DOM) — a modal that says one thing and waits to be dismissed.
//
// **Reported by the user on 2026-08-26**: a connection error arrived as a live-region
// announcement, like "output continues" or "nothing running to stop", and they expected
// something with an OK button. They were right. A failure is not a passing remark — it is
// the end of something they deliberately started, arriving seconds after they started it,
// and an announcement can be spoken over by whatever comes next or missed entirely if
// focus was moving. There is also nothing to go back to afterwards.
//
// The sentence is the dialog's description, so a reader says it as the dialog opens rather
// than leaving somebody to go and find it.

import { keepTabInside } from './dialog_tab';
import type { MessageView } from '../ports/message_view';

export class MessageDialog implements MessageView {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly body: HTMLElement,
  ) {
    // **Tab is answered here rather than left to the platform** — reported by the user on
    // 2026-08-26, who found Tab landing on OK again and again. With one control there is
    // nowhere to go, so the key is swallowed: no drop out of the dialog, and no reader
    // announcing the same button a second time.
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
  }

  /** Say it, and resolve once it has been dismissed. */
  show(sentence: string): Promise<void> {
    this.body.textContent = sentence;
    return new Promise<void>((resolve) => {
      const settle = (): void => {
        this.dialog.removeEventListener('close', settle);
        resolve();
      };
      this.dialog.addEventListener('close', settle);
      this.dialog.showModal();
      this.dialog.querySelector<HTMLElement>('#failed-ok')?.focus();
    });
  }
}
