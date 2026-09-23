// Role: adapter (DOM) — the dialog that holds a listener while a connection is being made.
//
// Escape closes it without cancelling the attempt, whose answer arrives whether this is
// showing or not.

export function connectingTo(label: string): string {
  return `connecting to ${label}`;
}

export class ConnectingDialog {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly what: HTMLElement,
  ) {}

  show(label: string): void {
    this.what.textContent = connectingTo(label);
    if (this.dialog.open) {
      return;
    }
    this.dialog.showModal();
  }

  hide(): void {
    if (this.dialog.open) {
      this.dialog.close();
    }
  }
}
