// Role: port (driving) — what the frontend may ask of one session.

import type {
  KeyAck,
  KeyPress,
  LineOwner,
  SessionEvent,
  SessionId,
  SubmitAck,
} from '../protocol';

export interface BackendApi {
  /**
   * Call after `ConnectApi.use` resolves; until then the session keeps at most `BACKLOG`
   * events (crates/acter-core/src/services/session.rs).
   */
  attachSession(
    session: SessionId,
    onEvent: (event: SessionEvent) => void,
  ): Promise<void>;
  /**
   * Answers `NotConnected` when nothing is behind this window or the session named has been
   * replaced; nothing was written and the line is still the caller's.
   */
  submitCommand(session: SessionId, line: string): Promise<SubmitAck>;
  sendKey(session: SessionId, key: KeyPress): Promise<KeyAck>;
  setLineOwner(session: SessionId, owner: LineOwner): Promise<void>;
  paste(session: SessionId, text: string): Promise<void>;
}
