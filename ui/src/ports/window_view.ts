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
}
