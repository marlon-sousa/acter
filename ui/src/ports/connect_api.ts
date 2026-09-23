// Role: port (driving) — what the frontend may ask about connecting: what this machine
// offers, which far end the window is on, and starting a different one.

import type {
  ConnectAnswer,
  ConnectQuestion,
  Connectable,
  Connected,
  LaunchRequest,
  ProfileId,
  SavedConnections,
  SetUp,
} from '../protocol';

/**
 * A far end that asks a question nobody is listening for is told nobody answered, which the
 * backend reads as a refusal.
 */
export interface ConnectListener {
  onQuestion?(question: ConnectQuestion): Promise<ConnectAnswer>;
  onProgress?(said: string): void;
}

export interface ConnectApi {
  connectable(): Promise<Connectable[]>;
  /**
   * Rejects with a whole spoken sentence when it cannot be started, and the running session
   * is untouched. `origin` is the saved connection this attempt started from, or `null`
   * for a new one.
   */
  use(
    id: ProfileId,
    setUp: SetUp,
    origin: string | null,
    listener?: ConnectListener,
  ): Promise<Connected>;
  /** `null` for a window connected to nothing. */
  connected(): Promise<Connected | null>;

  saved(): Promise<SavedConnections>;

  /** Answers the sentence to say, or rejects with one. */
  saveConnection(name: string): Promise<string>;

  /** Answers the sentence to say, or rejects with one. */
  renameConnection(from: string, to: string): Promise<string>;

  /** Answers the sentence to say, or rejects with one. */
  forgetConnection(name: string): Promise<string>;

  offerToSave(): Promise<boolean>;

  stopOfferingToSave(): Promise<void>;

  /** What `acter --connect <name>` asked for, or `null` for an ordinary launch. */
  requestedAtLaunch(): Promise<LaunchRequest | null>;
}
