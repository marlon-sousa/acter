// Role: port (driven) — what the controller needs in order to say what this window is.

export interface WindowView {
  /** `null` when it is not connected to one. */
  connectedTo(name: string | null): void;
  status(text: string): void;
  showTerminal(live: boolean): void;
  showLocalLine(showing: boolean): void;
}
