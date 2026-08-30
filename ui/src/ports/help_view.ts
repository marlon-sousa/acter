// Role: port (driving, view) — whoever can put the help topic in front of a person, at a
// named place in it.
//
// It exists because a second surface now opens help: F1 and the menu open it at the top,
// and the Connect dialog's Help button opens it at the section about the checkbox it sits
// beside (reported by the user on 2026-08-30). Neither caller knows what a dialog is, and
// the one that opens it from *inside* another dialog has somewhere of its own that focus
// must come back to — which is the whole of why this takes options rather than nothing.

export interface HelpView {
  /**
   * Show the topic.
   *
   * `topic` is the id of a heading inside it, and focus lands on that heading — a listener
   * arrives at the answer rather than at the top of a page they then have to search.
   * `returnTo` is what takes focus when it closes, for a caller that is itself a dialog and
   * would otherwise be left behind.
   */
  open(options?: { topic?: string; returnTo?: { focus(): void } }): void;
}
