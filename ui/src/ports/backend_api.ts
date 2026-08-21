// Role: port (driving) — what the frontend may ask of the backend.

import type { KeyAck, KeyPress, SessionEvent, SubmitAck } from '../protocol';

export interface BackendApi {
  /**
   * Establish the per-session event stream. `onEvent` is invoked for every
   * SessionEvent the backend emits (the inbound Channel path). Resolves once the
   * attach invoke has been acknowledged.
   */
  attachSession(onEvent: (event: SessionEvent) => void): Promise<void>;
  /**
   * Submit a line for execution. Resolves immediately with the correlation id every
   * later event about this command carries — an invoke never waits on the shell.
   */
  submitCommand(line: string): Promise<SubmitAck>;
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
  sendKey(key: KeyPress): Promise<KeyAck>;
}
