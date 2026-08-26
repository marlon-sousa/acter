// Role: port (driving) — what the frontend may ask of one session.
//
// **Every call carries the session id since B7**, and it is no longer a constant. A window
// can be connected to one far end and then another, so the id names *which* — a line
// submitted a moment before the user replaced their shell must not run in the new one, in
// a working directory and on a machine they never chose for it. Which session is current
// is `ConnectApi`'s answer, held by the controller and passed here.

import type { KeyAck, KeyPress, SessionEvent, SessionId, SubmitAck } from '../protocol';

export interface BackendApi {
  /**
   * Establish the per-session event stream. `onEvent` is invoked for every
   * SessionEvent the backend emits (the inbound Channel path). Resolves once the
   * attach invoke has been acknowledged.
   *
   * Called once per connection, after `ConnectApi.use` resolves — deliberately a separate
   * call, so the caller can clear a buffer still holding the previous shell's output
   * before any of the new one's arrives (spec B7, decision 1). Nothing said before the
   * attach is lost: the session holds what it said until somebody attaches (spec A9).
   */
  attachSession(
    session: SessionId,
    onEvent: (event: SessionEvent) => void,
  ): Promise<void>;
  /**
   * Submit a line for execution. Resolves immediately with the correlation id every
   * later event about this command carries — an invoke never waits on the shell.
   *
   * Answers `NotConnected` instead when there is nothing behind this window, or when the
   * session named here is one that has since been replaced. Nothing was written anywhere,
   * and the line is still the caller's to keep.
   */
  submitCommand(session: SessionId, line: string): Promise<SubmitAck>;
  /**
   * Report a keystroke the frontend did not consume, and learn what became of it.
   *
   * The key, not the meaning: the binding table lives in the domain, so a new binding
   * is a backend change with no frontend release. There is deliberately no
   * `interrupt()` beside this — a meaning-shaped method would put that table back on
   * this side of the wire.
   *
   * No command id: the session acts on whatever is running, because an id the frontend
   * supplied can only be stale by the time the invoke lands.
   */
  sendKey(session: SessionId, key: KeyPress): Promise<KeyAck>;
}
