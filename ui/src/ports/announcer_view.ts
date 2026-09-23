// Role: port (driven) — what the controller needs from the live-region announcer.
//
// Callers must render the text into the buffer before announcing it; the deferred drain
// preserves that order.

export interface AnnouncerView {
  announce(text: string): void;

  /**
   * Call after closing a modal dialog: a live region that has just returned to the
   * accessibility tree loses the first text change made to it, measured with NVDA.
   */
  documentReturned(): void;

  /**
   * Wait for this before opening a dialog after announcing: with NVDA 2026.1.1, what is
   * still queued when a dialog opens drains into the dialog's region, not the document's.
   */
  drained(): Promise<void>;
}
