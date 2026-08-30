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
   *
   * **In the same turn as `showModal`, and a hold here was measured and rejected.** Driving
   * NVDA 2026.1.1 as the `user` persona on 2026-08-30:
   *
   * - Focused in the same turn, the dialog announces its description and then the heading,
   *   which is what A13's decision 7 bought. What it does not do on the *first* opening is
   *   take the browse cursor with it: the first arrow press after arriving read "Close
   *   button" rather than the section, and arrowing *up* read the section correctly. The
   *   second opening, with the reader's view of the dialog already built, arrowed down into
   *   the section. That is the same lateness `window_chrome.ts` records for the window's
   *   first focus placement, and it is the reader's view catching up rather than the wrong
   *   element being focused.
   * - Focused a turn later (100 ms), the reader reached the open dialog with nothing focused
   *   inside it and **read the whole topic aloud** — the six-paragraph wall A13 added the
   *   description to stop — on every opening, not only the first.
   *
   * So the hold is not an improvement to make later: it is a worse trade, measured. What is
   * left is one lagging arrow press, once per window, against a topic read out in full every
   * time.
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
