// Role: port (driven) — what the controller needs from the live-region announcer.
//
// announce(text) enqueues; the adapter drains the queue one announcement per turn into
// the live region, so two back-to-back announcements are spoken as separate utterances
// rather than merged into one mutation batch (A5.2). Callers must render the text into
// the buffer before announcing it — the deferred drain preserves that order.
//
// documentReturned() is the other half of that, and it exists because of what a modal
// dialog does to a live region: while one is open the rest of the document is inert, and
// when it closes the region comes back with no history the reader can compare against.
// The first thing said into it is then lost (spec 13.3).

export interface AnnouncerView {
  announce(text: string): void;

  /**
   * Say that a modal dialog has closed, so what is announced next can be heard.
   *
   * **A live region that has just returned to the accessibility tree eats the first text
   * change made to it.** Measured 2026-08-30 through the screen-reader bridge: the sentence
   * naming the far end a connection reached went missing on six occasions across five NVDA
   * passes, and it was always the first thing said after the connect dialogs closed. Given
   * something else to lose first, it was heard in every one of ten connections.
   *
   * So this queues a change that carries text but no words — a baseline for the reader to
   * compare the next one against. Whoever closes a dialog calls it; it is not the closing
   * that needs announcing, it is the announcement after it that needs to survive.
   */
  documentReturned(): void;

  /**
   * Resolve once everything queued now has reached a live region.
   *
   * **Because a dialog that opens mid-sentence takes the sentence with it.** Found with
   * NVDA 2026.1.1 on 2026-09-12, driving a new connection as `user`: the two sentences a
   * connection says were queued, and the offer to save (spec 26, decision 19) opened while
   * the first was still in the queue — so the second drained into the *dialog's* region
   * rather than the document's, because the innermost open dialog is the only one anything
   * is listening to. It was heard that time. It is heard by accident, which is exactly the
   * shape of the defect 13.3 measured going missing six times in five passes.
   *
   * So whoever is about to open a dialog after announcing something waits for this. Only
   * the announcer can answer it: the queue and its spacing are its own, and a caller
   * guessing at a delay would be encoding that spacing in a second place.
   */
  drained(): Promise<void>;
}
