// Role: adapter (DOM) — the Help dialog: open it modally, and put focus back where the
// window keeps it when it closes.
//
// **The About dialog with its content already written**, and that is the point rather than
// an accident: the platform announces a modal `<dialog>` as a dialog, traps focus while it
// is open and closes it on Escape, so what is left is where focus belongs afterwards and
// the Tab the platform does not cycle (spec A7, decision 3).
//
// It reads nothing from the backend. What it explains is what a listener *hears*, which
// does not change with the build, the machine or the session — so the topic is static
// markup in views/main_window.html and this owns only the opening and closing of it.

import { keepTabInside } from './dialog_tab';
import type { HelpView } from '../ports/help_view';

export class HelpDialog implements HelpView {
  /**
   * Where the *current* opening came back to, which is the window unless whoever opened it
   * asked for somewhere else.
   *
   * **A dialog that opens this one has to be come back to**, and the window is not it: the
   * Connect dialog's Help button opens help on top of a dialog that is still there, and
   * sending focus to the window afterwards would send it somewhere inert and leave the
   * listener with nothing under them.
   */
  private comingBackTo: { focus(): void };

  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly returnTo: { focus(): void },
  ) {
    this.comingBackTo = returnTo;
    this.dialog.addEventListener('close', () => this.comingBackTo.focus());
    this.dialog
      .querySelector('#help-close')
      ?.addEventListener('click', () => this.dialog.close());
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
  }

  /**
   * Show the topic, or do nothing if it is already showing.
   *
   * The guard is About's and is needed twice over here. Opening an open dialog throws
   * `InvalidStateError` and the throw is silent, and this one has three ways in — F1, the
   * menu, and the Connect dialog's Help button — so "asked while already open" is an
   * ordinary thing rather than a double press.
   *
   * **Focus lands on the section that was asked for**, so a listener opening help from a
   * control arrives at the paragraph about that control rather than at the top of a topic
   * they then have to search. With no section asked for, the platform does what it did
   * before: the dialog announces its title and its one-line description, and the first Tab
   * finds Close.
   */
  open(options?: { topic?: string; returnTo?: { focus(): void } }): void {
    if (this.dialog.open) {
      return;
    }
    this.comingBackTo = options?.returnTo ?? this.returnTo;
    this.dialog.showModal();
    const topic = options?.topic;
    if (topic !== undefined) {
      this.dialog.querySelector<HTMLElement>(`#${topic}`)?.focus();
    }
  }
}
