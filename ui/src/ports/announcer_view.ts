// Role: port (driven) — what the controller needs from the live-region announcer.
//
// announce(text) enqueues; the adapter drains the queue one announcement per turn into
// the live region, so two back-to-back announcements are spoken as separate utterances
// rather than merged into one mutation batch (A5.2). Callers must render the text into
// the buffer before announcing it — the deferred drain preserves that order.
//
// settled() answers the other half of that deferral: whether anything is still owed. It
// exists because a caller can take the live region away — a dialog closing is a region
// leaving the accessibility tree — and words the reader has not taken yet go with it
// (spec 13.3).

export interface AnnouncerView {
  announce(text: string): void;

  /**
   * Resolves once everything announced so far has reached the reader.
   *
   * **Taken, not spoken.** The reader copies a live region's text into its own speech
   * queue when the change reaches it and utters it whenever it gets there — measured
   * 2026-08-30, where a marker put into a dialog's region was spoken 7 ms *after* that
   * region left the accessibility tree. So this resolving means the words are safe from
   * whatever the caller is about to do, not that the listener has heard them yet.
   *
   * Anything announced while a caller is awaiting this is waited for too: the promise is
   * about the queue's state when it resolves, not when it was asked for.
   */
  settled(): Promise<void>;
}
