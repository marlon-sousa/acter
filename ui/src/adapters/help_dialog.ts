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

export class HelpDialog {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly returnTo: { focus(): void },
  ) {
    this.dialog.addEventListener('close', () => this.returnTo.focus());
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
   * `InvalidStateError` and the throw is silent, and this one has two ways in — F1 and the
   * menu — so "asked while already open" is an ordinary thing rather than a double press.
   */
  open(): void {
    if (this.dialog.open) {
      return;
    }
    this.dialog.showModal();
  }
}
