// Role: adapter (DOM) — the dialog that holds a listener while a connection is being made.
//
// **Reported by the user on 2026-08-30**: pressing Enter on a connection kind put focus
// back on the list of kinds. Being returned to the control you have just acted on is what a
// dialog does when nothing happened, and a connection to a cold distribution takes five
// seconds — so the one moment a listener most needs to be told that something is under way
// was the moment the window said the least.
//
// What replaces it is this: Enter goes *forward*, into a dialog that names what is being
// connected to. The backend's own progress sentences land in its live region as the stages
// pass (spec B9, decision 6), so the wait is narrated rather than silent, and the answer —
// a session, a question to answer, or a failure to acknowledge — arrives on top of it.
//
// **It owns no decision.** It cannot cancel a connection, because nothing in this product
// can: an attempt in flight runs to its answer. So Escape is left to the platform, which
// closes it and puts the listener back on the Connect dialog underneath with its controls
// still unavailable — the attempt goes on, and its answer arrives whether this is showing
// or not. Refusing Escape would be the trap here, not the safeguard.

/** What it says it is doing, in the words the connection itself uses when it succeeds. */
export function connectingTo(label: string): string {
  return `connecting to ${label}`;
}

export class ConnectingDialog {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly what: HTMLElement,
  ) {}

  /**
   * Say that a connection to this far end is being made, and hold focus while it is.
   *
   * Nothing in here is focusable, so the platform focuses the dialog itself and a reader
   * announces its name and this sentence. That is deliberate: a paragraph is not a control
   * and does not belong in a tab order (the rule the set-up dialog was fixed under on
   * 2026-08-30), and there is nothing else here to put focus on.
   */
  show(label: string): void {
    this.what.textContent = connectingTo(label);
    if (this.dialog.open) {
      return;
    }
    this.dialog.showModal();
  }

  /**
   * Take it away: the attempt is over, whatever the answer was.
   *
   * Answers rather than throwing when it is already closed, because Escape can have closed
   * it while the attempt was still running.
   */
  hide(): void {
    if (this.dialog.open) {
      this.dialog.close();
    }
  }
}
