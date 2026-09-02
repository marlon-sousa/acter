// Role: port (driven) — what the controller needs in order to say what this window is.
//
// One port for both titles, because they are one fact: the operating system's title is what
// the desktop reads out in the task switcher, the heading is what a reader meets inside the
// document, and a window whose two titles disagree is worse than one that has neither
// (spec A9, decision 1).

export interface WindowView {
  /**
   * Name the far end this window is connected to, or `null` when it is not connected to
   * one. Both titles become `Acter` with nothing, `Acter - <name>` with something.
   */
  connectedTo(name: string | null): void;
  /** Say what the connection is doing, in words a listener hears when they change. */
  status(text: string): void;
  /**
   * Show the terminal window — the results buffer and the edit field — or show that there
   * is no session to show one for (spec A10).
   *
   * A window with nothing behind it holds a Connect button and no edit field: a buffer with
   * nothing in it and a field that can submit nothing are two controls a listener has to
   * arrow past to reach the only thing that would help them.
   *
   * The buffer is deliberately not part of this. It appears with its first content and
   * stays afterwards, because once a session has ended it is the record of what happened.
   */
  showTerminal(live: boolean): void;
  /**
   * Whether the window shows the local command line at all.
   *
   * **The two lines are never both in the document** (spec 28, decision 2). While the far
   * end owns the line the `<input>` owns nothing and can submit nothing, and leaving it
   * there would give a listener two edit fields to arrow between, only one of which does
   * anything — which is exactly the noise A10 took the field away to avoid.
   */
  showLocalLine(showing: boolean): void;
}
