// Role: adapter (DOM) — the Save connection dialog: one field holding the name, and the
// two shapes it comes in.
//
// **Saving happens from a live session and nowhere else** (spec 26, decision 15), so there
// is one way to save rather than two. This dialog is that way, and it is reached from two
// places that are the same place: File → Save connection at any time, and the offer Acter
// makes once after a new connection comes up.
//
// **The offering shape carries two more things** (decision 19): a paragraph saying the File
// menu can do this later, and a checkbox for somebody who never wants to be asked again.
// Its buttons read Save and Not now, because the question there is whether to save *this*
// one rather than whether to go on — and "Cancel" for an offer nobody asked for reads as
// cancelling something that was happening.
//
// **The name is prefilled and selected**, so typing replaces the suggestion and a listener
// who likes it can press Enter. The suggestion is the session's origin when it has one, and
// otherwise something that names the far end — composed here, because what a *suggestion*
// is called is a user-interface decision and the backend already refuses a bad one with its
// own sentence.

import { keepTabInside } from './dialog_tab';
import type { Connected } from '../protocol';

/** What the offering shape says about the other way to do this (decision 19). */
export const ALSO_LATER =
  'You can also save later from the File menu, under Save connection.';

/** What its checkbox says, as a whole sentence, because it is read aloud. */
export const NOT_AGAIN = 'Do not offer to save new connections';

/** What the dialog is for, said as it opens so a reader speaks it with the title. */
const WHY =
  'Give this connection a name, so you can start it again from the Connect dialog.';

/** And the same, when Acter is the one asking. */
const WHY_OFFERED =
  'This connection is working. Give it a name to save it, so you can start it again from ' +
  'the Connect dialog.';

/** What comes back: a name to save under, or nothing because they said no. */
export interface SaveAnswer {
  name: string | null;
  /** Whether they ticked "do not offer to save new connections" on the way past. */
  stopOffering: boolean;
}

export class SaveConnectionDialog {
  /** Whether this opening was Acter's idea, which decides the shape and the words. */
  private offering = false;
  private settle: ((answer: SaveAnswer) => void) | null = null;

  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly why: HTMLElement,
    private readonly field: HTMLInputElement,
    private readonly alsoLater: HTMLElement,
    private readonly notAgain: HTMLInputElement,
    private readonly save: HTMLButtonElement,
    private readonly cancel: HTMLButtonElement,
    /**
     * Where focus goes when this closes.
     *
     * **Found by the end-to-end suite, 2026-09-12.** This dialog opens over the *window*
     * rather than over another dialog — from the File menu, and by itself after a new
     * connection comes up — and without this, closing it left focus on nothing at all. The
     * platform restores focus to whatever had it when `showModal` ran, and at that moment
     * that is a control the window has since swapped out: a session hands the keys to the
     * program, so the local edit field is hidden, and focusing a hidden element is a no-op
     * (the lesson roadmap 28.3 and A10 both record). What a listener met was silence with
     * nowhere to arrow from.
     *
     * The window's own landing decides where, which is the same object every other dialog
     * here is handed: whichever command line is in front, or the Connect button when there
     * is no session.
     */
    private readonly returnTo: { focus(): void },
  ) {
    this.dialog.addEventListener('keydown', (event) => keepTabInside(this.dialog, event));
    // **Enter saves, from the field.** The whole dialog is one text box and two buttons,
    // and a listener who has typed a name and pressed Enter has said what they want — this
    // is not a consequential choice where Enter should reach nothing (ARCHITECTURE, dialogs
    // rule 8: that rule is for decisions where either outcome is serious).
    this.dialog.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' && (event.target as HTMLElement).closest('button') === null) {
        event.preventDefault();
        this.answer(this.field.value);
      }
    });
    this.save.addEventListener('click', () => this.answer(this.field.value));
    this.cancel.addEventListener('click', () => this.answer(null));
    // Escape, and every other way of closing, is "not now": nothing is saved and the
    // preference is still recorded if the box was ticked, because ticking it is a decision
    // about the *offer* rather than about this connection (decision 19).
    this.dialog.addEventListener('close', () => {
      this.answer(null);
      this.returnTo.focus();
    });
  }

  /**
   * Ask for a name, with the field prefilled and selected.
   *
   * `offering` is whether Acter is the one asking, which is the once-after-connecting case
   * (decision 19); `false` is File → Save connection, where the paragraph and the checkbox
   * are not there at all.
   */
  ask(suggestion: string, offering: boolean): Promise<SaveAnswer> {
    this.offering = offering;
    this.why.textContent = offering ? WHY_OFFERED : WHY;
    this.alsoLater.textContent = ALSO_LATER;
    this.alsoLater.hidden = !offering;
    this.notAgain.checked = false;
    // The checkbox and its label travel together, which is what `hidden` on the paragraph
    // around them buys: a control removed from the document is a control Tab cannot reach
    // and a reader cannot meet.
    const around = this.notAgain.closest('p');
    if (around !== null) {
      around.hidden = !offering;
    }
    this.save.textContent = 'Save';
    // **"Not now" rather than "Cancel"** when Acter asked (decision 19): there is nothing
    // to cancel, and a listener who meets Cancel on an offer they did not ask for is being
    // told they interrupted something.
    this.cancel.textContent = offering ? 'Not now' : 'Cancel';
    this.field.value = suggestion;
    return new Promise<SaveAnswer>((resolve) => {
      this.settle = resolve;
      this.dialog.showModal();
      // **Focus lands on the field with its text selected**, so typing replaces the
      // suggestion (decision 18). It is what the dialog is *for*, which is where focus
      // opens (ARCHITECTURE, dialogs rule 8).
      this.field.focus();
      this.field.select();
    });
  }

  /**
   * A refusal keeps the dialog open, with the sentence announced and focus back in the
   * field (decision 18) — which is where trying something else begins (ARCHITECTURE,
   * dialogs rule 11).
   */
  refused(): void {
    this.field.focus();
    this.field.select();
  }

  /** Whether it is still up, which is how the caller knows a refusal left it open. */
  get open(): boolean {
    return this.dialog.open;
  }

  /** Close it for good, which is what a save the backend accepted does. */
  finish(): void {
    this.settle = null;
    if (this.dialog.open) {
      this.dialog.close();
    }
  }

  private answer(name: string | null): void {
    const settle = this.settle;
    if (settle === null) {
      return;
    }
    this.settle = null;
    const stopOffering = this.offering && this.notAgain.checked;
    if (name === null) {
      // Said no, so the dialog goes; a name is answered with the dialog still up, because
      // the backend may refuse it and this is where trying again happens.
      if (this.dialog.open) {
        this.dialog.close();
      }
    }
    settle({ name, stopOffering });
  }
}

/**
 * What to prefill the field with for a session nobody has named (decision 18).
 *
 * **The session's origin when it has one**, which is the File → Save connection case after
 * somebody changed a port — the name they already know it by. Otherwise something that
 * names the far end, so the suggestion is a name they would have typed rather than a
 * placeholder they have to replace.
 */
export function suggestion(connected: Connected): string {
  if (connected.saved_as !== null) {
    return connected.saved_as;
  }
  // **The connect list's own label, with the category taken off.** "SSH: marlon at
  // example.org" is what the window is called; what somebody would type as a name is the
  // half after the colon, because the category is the one part they already know — which
  // gives exactly the suggestions decision 18 lists: "marlon at example.org" for SSH, the
  // distribution's name for WSL, "PowerShell 7", "Command Prompt", and the scenario's name
  // for a scripted session.
  const after = connected.label.indexOf(': ');
  return after === -1 ? connected.label : connected.label.slice(after + 2);
}
