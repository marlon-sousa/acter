// Role: port (driven) — what the controller needs from the live-region announcer.
//
// announce(text) enqueues; the adapter drains the queue one announcement per turn into
// the live region, so two back-to-back announcements are spoken as separate utterances
// rather than merged into one mutation batch (A5.2). Callers must render the text into
// the buffer before announcing it — the deferred drain preserves that order.

export interface AnnouncerView {
  announce(text: string): void;
}
