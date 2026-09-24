// Role: controller — translates UI intents into backend calls and renders the backend's
// SessionEvent stream into the buffer, live region, and beep.

import type {
  Announcement,
  Connectable,
  Connected,
  ConnectionState,
  CommandId,
  KeyAck,
  KeyPress,
  LineOwner,
  ProfileId,
  SavedConnections,
  SessionEvent,
  SessionId,
  SetUp,
} from '../protocol';
import type { AnnouncerView } from '../ports/announcer_view';
import type { BackendApi } from '../ports/backend_api';
import type { BeepView } from '../ports/beep_view';
import type { BufferView } from '../ports/buffer_view';
import type { ConnectApi } from '../ports/connect_api';
import type { MessageView } from '../ports/message_view';
import type { QuestionView } from '../ports/question_view';
import type { WindowView } from '../ports/window_view';
import type { EditFieldView } from '../ports/edit_field_view';
import type { FarEndFieldView } from '../ports/far_end_field_view';

export interface SaveOffer {
  (connected: Connected): Promise<{ stopOffering: boolean }>;
}

// Spoken while a long command runs.
export const patienceMessage =
  'long command running, output is accumulating in the buffer';
// Spoken when a program enters the alternate screen.
export const altScreenEnteredMessage =
  'this program needs interactive mode, which is not available yet. Press Ctrl+C to return to the prompt';
// Spoken when a program leaves the alternate screen.
export const altScreenLeftMessage = 'interactive program ended';
// Spoken when the session's shell never announced itself.
export const integrationUnavailableMessage =
  'You will hear what commands print here, but not whether they worked. Press F1 for help.';
// Spoken when output is no longer read aloud; it still reaches the buffer.
export const outputContinuesMessage =
  'output continues, accumulating in the buffer without being read';
// Spoken when a command's output is too big to read.
export function tooBigMessage(lineCount: number): string {
  return `${lineCount} lines arrived, too big to read`;
}
// Spoken when a command fails.
export function failureMessage(exitCode: number): string {
  return `command failed, exit code ${exitCode}`;
}
// Spoken when a key has nothing running to act on.
export const nothingToStopMessage = 'nothing running to stop';

// The status region's words, one per connection state.
export const connectingStatus = 'connecting';
export const disconnectedStatus = 'not connected';

/** The status region's words and the connection announcement are this one string. */
export function connectedStatus(label: string, note?: string | null): string {
  const said = `connected to ${label}`;
  return note === undefined || note === null ? said : `${said}, ${note}`;
}

// Spoken whenever the window has no session to act on.
export const notConnectedMessage = 'not connected. Choose Connect to start a shell';
// Spoken when a connection starts.
export function connectedMessage(label: string, note?: string | null): string {
  return connectedStatus(label, note);
}

// Spoken for a key the far end does not act on.
export const unboundKeyMessage = 'that key does nothing here';

// Spoken by File then Save connection with nothing connected; the same words as
// `NOTHING_TO_SAVE` in crates/acter-core/src/services/connect.rs.
export const nothingToSaveMessage =
  'Nothing is connected, so there is nothing to save.';

// Spoken when the keyboard is handed to the far end, and back.
export const farEndLineOnMessage = 'Remote process keys.';
export const farEndLineOffMessage = 'Acter process keys.';

// Spoken after every connection: the reader does not say which line focus landed on.
export const keysGoToTheProgramMessage =
  'Remote process keys. Ctrl+Shift+K changes that.';
export const keysGoToActerMessage = 'Acter process keys. Ctrl+Shift+K changes that.';

// Spoken for Ctrl+D in a shell with no end-of-input key.
export const noEndOfInputKeyMessage =
  'This shell has no key for end of input. Type exit and press Enter.';
// Spoken for Ctrl+D when the session has already ended.
export const sessionAlreadyEndedMessage = 'This session has already ended.';

/** `null` when nothing is to be said. */
function answerTo(press: KeyPress, ack: KeyAck): string | null {
  const endOfInput =
    typeof press.key === 'object' &&
    press.key.Char === 'd' &&
    press.ctrl &&
    !press.shift &&
    !press.alt;
  switch (ack) {
    case 'Applied':
      return null;
    case 'NothingToActOn':
      return endOfInput ? sessionAlreadyEndedMessage : nothingToStopMessage;
    case 'Unsupported':
      return endOfInput ? noEndOfInputKeyMessage : unboundKeyMessage;
    case 'Unbound':
      return unboundKeyMessage;
    default:
      return assertNeverAck(ack);
  }
}

function assertNever(event: never): never {
  throw new Error(`unhandled SessionEvent variant: ${JSON.stringify(event)}`);
}

function assertNeverAck(ack: never): never {
  throw new Error(`unhandled KeyAck variant: ${JSON.stringify(ack)}`);
}

function assertNeverAnnouncement(announcement: never): never {
  throw new Error(
    `unhandled Announcement kind: ${JSON.stringify(announcement)}`,
  );
}

/** Tauri rejects with the backend's `Err` string, already a whole spoken sentence. */
function reason(why: unknown): string {
  if (typeof why === 'string' && why.trim() !== '') {
    return why;
  }
  return `the connection could not be started: ${String(why)}`;
}

export class AppController {
  private readonly openBlocks = new Set<CommandId>();
  private readonly tooBig = new Set<CommandId>();
  private readonly echoed = new Set<CommandId>();
  // Whether the connection announcement already said this session has no shell
  // integration; reset per connection.
  private noteSaidIntegrationIsMissing = false;
  // null for a window connected to nothing.
  private session: SessionId | null = null;
  private connection: Connected | null = null;
  // Decides only what this window shows and where focus goes; the backend holds the state
  // that decides what a key does.
  private lineOwner: LineOwner = 'Local';

  constructor(
    private readonly backend: BackendApi,
    private readonly connect: ConnectApi,
    private readonly editField: EditFieldView,
    private readonly buffer: BufferView,
    // Without it the window never leaves the local line.
    private readonly farEndField: FarEndFieldView | undefined,
    private readonly announcer: AnnouncerView,
    private readonly beep: BeepView,
    private readonly window: WindowView,
    private readonly questions?: QuestionView,
    private readonly failure?: MessageView,
  ) {}

  async start(): Promise<void> {
    await this.show(await this.connect.connected());
  }

  connectable(): Promise<Connectable[]> {
    return this.connect.connectable();
  }

  /**
   * Answers whether the window is on the new far end; on failure the running session is
   * still running and attached.
   */
  async connectTo(
    id: ProfileId,
    setUp: SetUp = 'Yes',
    origin: string | null = null,
  ): Promise<boolean> {
    let connected: Connected;
    try {
      connected = await this.connect.use(id, setUp, origin, {
        onProgress: (said) => this.announcer.announce(said),
        onQuestion: (question) =>
          this.questions === undefined
            ? Promise.resolve({ answer: 'GiveUp' as const })
            : this.questions.ask(question),
      });
    } catch (why) {
      const said = reason(why);
      if (this.failure === undefined) {
        this.announcer.announce(said);
      } else {
        await this.failure.show(said);
      }
      return false;
    }
    await this.show(connected);
    return true;
  }

  saved(): Promise<SavedConnections> {
    return this.connect.saved();
  }

  async offerToSave(ask: SaveOffer): Promise<void> {
    const connected = this.connection;
    if (connected === null || connected.saved_as !== null) {
      return;
    }
    if (!(await this.connect.offerToSave())) {
      return;
    }
    await this.announcer.drained();
    const answer = await ask(connected);
    if (answer.stopOffering) {
      await this.connect.stopOfferingToSave();
    }
  }

  /** `null` when refused, and the refusal has already been announced. */
  async saveConnection(name: string): Promise<string | null> {
    try {
      const said = await this.connect.saveConnection(name);
      if (this.connection !== null) {
        this.connection = { ...this.connection, saved_as: name.trim() };
      }
      return said;
    } catch (why) {
      this.announcer.announce(reason(why));
      return null;
    }
  }

  /** Answers whether the window is now on the connection the launch asked for. */
  async carryOutTheLaunchSwitch(): Promise<boolean> {
    const asked = await this.connect.requestedAtLaunch();
    if (asked === null) {
      return false;
    }
    if (asked.request === 'Unknown') {
      this.announcer.announce(asked.said);
      return false;
    }
    const rows = (await this.connect.saved()).rows;
    const row = rows.find((saved) => saved.name === asked.name);
    if (row === undefined) {
      // The same words acter-core's `protocol_commands.rs` gives this refusal.
      this.announcer.announce(`There is no saved connection named ${asked.name}.`);
      return false;
    }
    return await this.connectTo(row.id, row.set_up, row.name);
  }

  /** Answers whether there was anything to say. */
  announceConnection(): boolean {
    if (this.connection === null) {
      return false;
    }
    this.announcer.announce(
      connectedMessage(this.connection.label, this.connection.note),
    );
    this.announcer.announce(
      this.lineOwner === 'FarEnd'
        ? keysGoToTheProgramMessage
        : keysGoToActerMessage,
    );
    return true;
  }

  get connectedNow(): Connected | null {
    return this.connection;
  }

  /**
   * Clears the buffer before attaching, so the new session's output never lands under the
   * old one's.
   */
  private async show(connected: Connected | null): Promise<void> {
    this.noteSaidIntegrationIsMissing = connected?.limit_explained ?? false;
    this.setLineOwner('Local', false);
    this.buffer.clear();
    this.openBlocks.clear();
    this.tooBig.clear();
    this.echoed.clear();
    this.session = connected === null ? null : connected.session;
    // Before the attach, so nothing the new session says reaches a window still shaped for
    // the last one.
    this.window.showTerminal(connected !== null);

    if (connected === null) {
      this.connection = null;
      this.window.connectedTo(null);
      this.window.status(disconnectedStatus);
      this.announcer.announce(notConnectedMessage);
      return;
    }
    this.window.connectedTo(connected.label);
    this.connection = connected;
    this.window.status(connectedStatus(connected.label, connected.note));
    // Before the attach, so the owner of the line is set from the far end's first byte.
    await this.backend.setLineOwner(connected.session, connected.line_owner);
    this.setLineOwner(connected.line_owner, false);
    await this.backend.attachSession(connected.session, (event) => {
      this.handleEvent(event);
    });
  }

  async submit(): Promise<void> {
    const text = this.editField.value().trim();
    if (this.session === null) {
      this.announcer.announce(notConnectedMessage);
      return;
    }
    const ack = await this.backend.submitCommand(this.session, text);
    if (ack.status === 'NotConnected') {
      this.announcer.announce(notConnectedMessage);
      return;
    }
    if (text === '') {
      this.editField.clear();
      return;
    }
    // An event may have opened this block before the ack resolved.
    this.buffer.openBlock(
      ack.command_id,
      this.echoed.has(ack.command_id) ? '' : text,
    );
    this.openBlocks.add(ack.command_id);
    this.editField.clear();
  }

  // `null` means the shell did not say which line this block is running.
  private headByTheEcho(commandId: CommandId, commandLine: string | null): void {
    if (commandLine === null || commandLine === '') {
      return;
    }
    this.echoed.add(commandId);
    this.buffer.openBlock(commandId, commandLine);
  }

  private ensureBlock(commandId: CommandId): void {
    if (!this.openBlocks.has(commandId)) {
      this.buffer.openBlock(commandId, '');
      this.openBlocks.add(commandId);
    }
  }

  private handleEvent(event: SessionEvent): void {
    switch (event.type) {
      case 'CommandStarted':
        this.ensureBlock(event.command_id);
        this.headByTheEcho(event.command_id, event.command_line);
        break;
      case 'Output':
        this.ensureBlock(event.command_id);
        this.buffer.applyLine(
          event.command_id,
          event.line,
          event.revision,
          event.text,
          event.prompt,
        );
        break;
      case 'CommandFinished':
        this.ensureBlock(event.command_id);
        if (this.tooBig.has(event.command_id)) {
          this.beep.beep();
        }
        this.tooBig.delete(event.command_id);
        this.openBlocks.delete(event.command_id);
        this.echoed.delete(event.command_id);
        break;
      case 'PromptDrawn':
        this.buffer.appendPrompt(event.text);
        this.announcer.announce(event.text);
        break;
      case 'CommandInterrupted':
        this.ensureBlock(event.command_id);
        this.tooBig.delete(event.command_id);
        this.openBlocks.delete(event.command_id);
        this.echoed.delete(event.command_id);
        break;
      case 'IntegrationUnavailable':
        if (!this.noteSaidIntegrationIsMissing) {
          this.announcer.announce(integrationUnavailableMessage);
        }
        break;
      case 'AltScreenEntered':
        this.announcer.announce(altScreenEnteredMessage);
        break;
      case 'AltScreenLeft':
        this.announcer.announce(altScreenLeftMessage);
        break;
      case 'Announce':
        this.handleAnnouncement(event.command_id, event.announcement);
        break;
      case 'ConnectionChanged':
        this.connectionChanged(event.state);
        break;
      case 'FarEndLine':
        // Not announced: NVDA speaks the text box's own changes, and a live region
        // answering as well produced two utterances, measured with NVDA.
        this.farEndField?.render(event.text, event.caret, event.anchored, this.completing);
        this.completing = false;
        break;
      case 'TitleChanged':
        break;
      default:
        assertNever(event);
    }
  }

  private connectionChanged(state: ConnectionState): void {
    switch (state) {
      case 'Connecting':
        this.window.status(connectingStatus);
        break;
      case 'Connected':
        if (this.connection !== null) {
          this.window.status(
            connectedStatus(this.connection.label, this.connection.note),
          );
        }
        break;
      case 'Reconnecting':
        this.window.status(connectingStatus);
        break;
      case 'Disconnected':
        this.window.status(disconnectedStatus);
        this.connection = null;
        this.window.connectedTo(null);
        this.session = null;
        this.setLineOwner('Local', false);
        this.window.showTerminal(false);
        this.announcer.announce(notConnectedMessage);
        break;
      default:
        assertNever(state);
    }
  }

  private handleAnnouncement(
    commandId: CommandId,
    announcement: Announcement,
  ): void {
    switch (announcement.kind) {
      case 'ReadAloud':
        this.announcer.announce(announcement.text);
        break;
      case 'TooBig':
        this.tooBig.add(commandId);
        this.announcer.announce(tooBigMessage(announcement.lines));
        break;
      case 'StillRunning':
        this.announcer.announce(patienceMessage);
        break;
      case 'OutputContinues':
        this.announcer.announce(outputContinuesMessage);
        break;
      case 'Failed':
        this.announcer.announce(failureMessage(announcement.exit_code));
        break;
      default:
        assertNeverAnnouncement(announcement);
    }
  }

  toggleFocusArea(): void {
    const line = this.commandLine();
    if (line.isFocused()) {
      this.buffer.focus();
    } else {
      line.focus();
    }
  }

  escapeToCommandLine(): void {
    if (this.buffer.containsFocus()) {
      this.commandLine().focus();
    }
  }

  private commandLine(): { focus(): void; isFocused(): boolean } {
    return this.lineOwner === 'FarEnd' && this.farEndField !== undefined
      ? this.farEndField
      : this.editField;
  }

  // Set on every reported key, so a Tab that brought no answer cannot mark the next key's.
  private completing = false;

  async reportKey(press: KeyPress): Promise<void> {
    this.completing = press.key === 'Tab';
    if (this.session === null) {
      this.announcer.announce(
        answerTo(press, 'NothingToActOn') ?? nothingToStopMessage,
      );
      return;
    }
    const ack = await this.backend.sendKey(this.session, press);
    const said = answerTo(press, ack);
    if (said !== null) {
      this.announcer.announce(said);
    }
  }

  async toggleLineOwner(): Promise<void> {
    if (this.session === null || this.farEndField === undefined) {
      this.announcer.announce(notConnectedMessage);
      return;
    }
    const next: LineOwner = this.lineOwner === 'Local' ? 'FarEnd' : 'Local';
    await this.backend.setLineOwner(this.session, next);
    this.setLineOwner(next, true);
  }

  async pasteToFarEnd(text: string): Promise<void> {
    if (this.session === null || this.lineOwner !== 'FarEnd') {
      return;
    }
    await this.backend.paste(this.session, text);
  }

  private setLineOwner(owner: LineOwner, announce: boolean): void {
    this.lineOwner = owner;
    if (this.farEndField === undefined) {
      return;
    }
    const far = owner === 'FarEnd';
    this.farEndField.show(far);
    this.window.showLocalLine(!far);
    if (far) {
      this.farEndField.focus();
    } else if (announce) {
      this.editField.focus();
    }
    if (announce) {
      this.announcer.announce(far ? farEndLineOnMessage : farEndLineOffMessage);
    }
  }

  editFieldHasSelection(): boolean {
    return this.editField.hasSelection();
  }
}
