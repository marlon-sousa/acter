// Role: port (driving) — what the frontend may ask about connecting: what this machine
// offers, which far end the window is on, and starting a different one.
//
// Separate from `BackendApi` because it is a different conversation. That port is one
// session's input and output and every call carries a session id; this one is about
// *which* session there is. Keeping them apart is also what stops a connect dialog from
// being able to submit commands.
//
// The backend actions behind this are named and testable without a window (spec B7), which
// is the whole reason connecting is not simply a menu handler: nothing in this project can
// drive a menu end to end, so the menu became the thinnest possible caller of something
// that can be driven.

import type { Connectable, Connected, ProfileId } from '../protocol';

export interface ConnectApi {
  /**
   * Everything this machine offers, asked afresh. Includes what cannot be started, last
   * and labelled, each carrying what to do about it — a list that silently omitted WSL
   * would teach a listener that Acter does not support it.
   */
  connectable(): Promise<Connectable[]>;
  /**
   * Start this one and replace whatever was running.
   *
   * **Rejects with a whole spoken sentence** when it cannot be started, and the session
   * that was running is untouched — still running, still attached. The caller says the
   * sentence and carries on.
   */
  use(id: ProfileId): Promise<Connected>;
  /** Which far end this window is on, or `null` for a window connected to nothing. */
  connected(): Promise<Connected | null>;
}
