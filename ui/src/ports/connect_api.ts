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

import type {
  ConnectAnswer,
  ConnectQuestion,
  Connectable,
  Connected,
  ProfileId,
  SetUp,
} from '../protocol';

/**
 * What a caller wants to hear about while a connection is being made.
 *
 * **Both are optional, and a caller that supplies neither still connects** — to anything
 * that does not ask questions, which is every far end except SSH. One that does ask and
 * finds nobody listening is told nobody answered, which the backend reads as a refusal
 * (spec B9, decision 3): Acter never trusts a host key because there was no one to object.
 */
export interface ConnectListener {
  /**
   * A question that has to be answered before connecting can go on: a host key to trust or
   * refuse, a password to give. Resolving with an answer lets the connection continue.
   */
  onQuestion?(question: ConnectQuestion): Promise<ConnectAnswer>;
  /**
   * Something worth saying while it happens. A listener with no feedback cannot tell a slow
   * network from a dead one (spec B9, decision 6).
   */
  onProgress?(said: string): void;
}

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
   *
   * `setUp` is the Connect dialog's checkbox: whether this connection may run one command
   * inside the session once it is established, so a listener gets a heading for each command
   * and is told when one fails (spec B9.5, decision 9). It travels with the attempt rather
   * than being stored, because there is no profile store to keep it in until B8.
   */
  use(id: ProfileId, setUp: SetUp, listener?: ConnectListener): Promise<Connected>;
  /** Which far end this window is on, or `null` for a window connected to nothing. */
  connected(): Promise<Connected | null>;
}
