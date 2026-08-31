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
}
