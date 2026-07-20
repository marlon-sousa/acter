// Role: port (driving) — what the frontend may ask of the backend.

import type { SessionEvent, SubmitAck } from '../protocol';

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
}
