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
  LaunchRequest,
  ProfileId,
  SavedConnections,
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
   * inside the session once it is established, so a listener is told when each command has
   * finished and, where the shell can say it, whether it worked (spec B9.5, decision 9). It
   * travels with the attempt, and is written down only when somebody saves the connection.
   *
   * `origin` is the saved connection this attempt started from, or `null` for a new one
   * (spec 26, decision 11). It is this side's knowledge, because the user may have edited
   * the panel before pressing Connect and the backend cannot know which row that came from
   * — and it decides two things: who holds the line as the session opens, and whether the
   * window offers to save it.
   */
  use(
    id: ProfileId,
    setUp: SetUp,
    origin: string | null,
    listener?: ConnectListener,
  ): Promise<Connected>;
  /** Which far end this window is on, or `null` for a window connected to nothing. */
  connected(): Promise<Connected | null>;

  /**
   * Every saved connection, freshly read, with the sentence to say instead when the
   * document could not be parsed (spec 26, decisions 9 and 11).
   *
   * **Rows rather than a document.** Nothing here reads or writes the settings file: the
   * backend answers typed rows and takes named actions, which is what makes a new setting
   * a compile error rather than a silent nothing.
   */
  saved(): Promise<SavedConnections>;

  /**
   * Write the live session down under this name, and answer the sentence to say.
   *
   * **Rejects with a sentence** when the name is taken, breaks the name rule, or nothing is
   * connected — the same shape `use` has, and the same reason: the words are the backend's,
   * because only it knows what was wrong.
   */
  saveConnection(name: string): Promise<string>;

  /** Give a saved connection a different name, and answer the sentence to say. */
  renameConnection(from: string, to: string): Promise<string>;

  /** Remove one, and answer the sentence to say. Nobody can undo this. */
  forgetConnection(name: string): Promise<string>;

  /**
   * Whether a new connection that has just come up should offer to save itself
   * (spec 26, decision 19).
   *
   * **Asked at the moment of the offer** rather than at startup, so a preference set in
   * another window is honoured without a restart.
   */
  offerToSave(): Promise<boolean>;

  /** Record that it should not, which is the offer's own checkbox and nothing else. */
  stopOfferingToSave(): Promise<void>;

  /**
   * What `acter --connect <name>` asked for, or `null` for an ordinary launch (spec 26,
   * decision 20).
   *
   * **The window is what carries it out**, through the same call the Connect dialog makes,
   * so a saved SSH connection asks its host-key and password questions in front of the
   * person who can answer them.
   */
  requestedAtLaunch(): Promise<LaunchRequest | null>;
}
