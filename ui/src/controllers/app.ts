// Role: controller — translates UI intents into backend calls and renders the
// backend's SessionEvent stream into the buffer, live region, and beep. Framework-free:
// sees only the BackendApi port and the view ports.

import type {
  Announcement,
  Connectable,
  Connected,
  ConnectionState,
  CommandId,
  KeyPress,
  ProfileId,
  SessionEvent,
  SessionId,
} from '../protocol';
import type { AnnouncerView } from '../ports/announcer_view';
import type { BackendApi } from '../ports/backend_api';
import type { BeepView } from '../ports/beep_view';
import type { BufferView } from '../ports/buffer_view';
import type { ConnectApi } from '../ports/connect_api';
import type { QuestionView } from '../ports/question_view';
import type { WindowView } from '../ports/window_view';
import type { EditFieldView } from '../ports/edit_field_view';

// Pinned announcement strings (spec decision 3). Every announced string is a domain
// requirement; this module is their single source in the frontend. The dynamic ones
// (N, exit code) are functions so the wording lives in exactly one place.
export const patienceMessage =
  'long command running, output is accumulating in the buffer';
export const altScreenEnteredMessage =
  'this program needs interactive mode, which is not available yet. Press Ctrl+C to return to the prompt';
export const altScreenLeftMessage = 'interactive program ended';
// This session's shell never announced itself, so there are no command boundaries in it:
// nothing is read aloud, output still accumulates in the buffer for review, and long
// commands are still announced as running. The wording says what the user has to do
// differently rather than naming the mechanism, because "OSC 133" is not a thing to say
// to somebody trying to run a command (DESIGN's reliability case 2, backend event added
// in B6).
export const integrationUnavailableMessage =
  'shell integration unavailable, output will not be read automatically; review it in the buffer';
// The babble guard tripped: output keeps arriving and keeps reaching the buffer, it is
// simply no longer read aloud. The wording says both halves, because "quiet" here never
// means the output stopped or was withheld (DESIGN, buffer and speech are separate).
export const outputContinuesMessage =
  'output continues, accumulating in the buffer without being read';
export function tooBigMessage(lineCount: number): string {
  return `${lineCount} lines arrived, too big to read`;
}
export function failureMessage(exitCode: number): string {
  return `command failed, exit code ${exitCode}`;
}
// The two answers to a keystroke that only the frontend can voice (A3.2 decision 7).
// `Applied` is still deliberately absent, for a different reason since B4.1: an interrupt
// that was accepted has an answer coming from the far end, and it is a better answer than
// anything Acter could say. The shell's prompt reappearing is read by autoread like any
// other output, and it is evidence rather than a claim — so Acter says nothing of its own
// about a stop, exactly as `CommandFinished` has carried no verdict since A6.
//
// `nothingToStopMessage` stays, and the same reasoning is why: with nothing running there
// is no shell output coming, so silence would be the only answer the user got.
//
// A3.1 decision 6 named this one: the typed `stop` had no honest way to say it, because
// the only surface that could justify the words is a key with an ack to report them.
export const nothingToStopMessage = 'nothing running to stop';

// What the status region says, one string per state (spec A9, decision 2). Pinned here with
// the rest of this frontend's user-facing words, because every one of them is read aloud and
// is therefore a domain requirement rather than presentation.
//
// "connecting" is the one that had to exist: a window opening onto a shell that takes
// seconds to start used to say nothing at all, and a listener cannot tell a slow start from
// a broken one (roadmap 23.7).
export const connectingStatus = 'connecting';
export const connectedStatus = 'connected';
export const disconnectedStatus = 'not connected';

// The two strings the unconnected window needs (spec B7, decision 3).
//
// **The first is said twice on purpose**: once when a window opens onto nothing, and again
// for every line submitted into it. Hearing the same words is how a user learns that this
// is one state rather than two problems — and a line typed into an empty window has to be
// *answered*, because silence is indistinguishable from a shell that is thinking.
//
// **It stopped naming a keystroke with A10.** B7's window told the listener to press F10 and
// find Connect in a menu, because a menu was the only way in; the window now opens with a
// Connect button under focus, so the route to describe is the control they are already on.
export const notConnectedMessage = 'not connected. Choose Connect to start a shell';
// And what a listener hears when one starts, which is the connect list's own label — so
// what they chose and what the window now calls itself are the same words (spec A9).
//
// **One sentence rather than two, since B9** (decision 7). A far end that has something more
// to say about itself says it here, in the same utterance: "connected to SSH: acter at
// acter-ssh, bash, with no shell integration set up on this host". The alternative was the
// name arriving now and the integration state arriving a second or two later, interrupting
// whatever was being said — an asynchronous fact speaking over the thing it describes.
export function connectedMessage(label: string, note?: string | null): string {
  const said = `connected to ${label}`;
  return note === undefined || note === null ? said : `${said}, ${note}`;
}

// Unreachable while Ctrl+C is both the only key reported and the only key bound. It is
// still spoken, because the first thing a second reported key must not do is vanish.
export const unboundKeyMessage = 'that key does nothing here';

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

/**
 * The sentence out of a rejected connect.
 *
 * Tauri rejects with the `Err` string the backend wrote, which is already a whole spoken
 * sentence. Anything else reaching here is a bug rather than a far end that would not
 * start, and it still has to be *said*: a listener meeting silence has no way to tell a
 * broken connect from one that is taking its time.
 */
function reason(why: unknown): string {
  if (typeof why === 'string' && why.trim() !== '') {
    return why;
  }
  return `the connection could not be started: ${String(why)}`;
}

export class AppController {
  // Commands with an open buffer block, so an event can find (or lazily open) one.
  private readonly openBlocks = new Set<CommandId>();
  // Commands that have carried a too-big chunk, so the completion beep fires on their
  // finish. View state the frontend legitimately owns (decision 2).
  private readonly tooBig = new Set<CommandId>();
  // Commands whose heading came from the shell's own echo, so the optimistic heading
  // from a submit ack never overwrites it (spec B6.1, decision 1).
  private readonly echoed = new Set<CommandId>();
  // Whether the connection announcement already said this session has no shell
  // integration, so the session's own announcement of it is not said a second time.
  //
  // **The two say the same thing and only one of them can name the far end** (spec B9,
  // decision 7). `IntegrationUnavailable` arrives when the startup grace period expires with
  // no markers, which for an SSH session is always — and by then the connection has already
  // said "bash, with no shell integration set up on this host", which is the same fact with
  // a subject. Hearing it twice teaches a listener that Acter repeats itself.
  //
  // Reset per connection rather than latched, so a later session that is unintegrated for
  // its own reasons still says so.
  private noteSaidIntegrationIsMissing = false;
  // Which session every invoke names, or null for a window connected to nothing.
  //
  // Held here rather than as a constant in the router since B7: a window can be connected
  // to one far end and then another, and a line submitted a moment before that happened
  // must not run in the new one.
  private session: SessionId | null = null;

  constructor(
    private readonly backend: BackendApi,
    private readonly connect: ConnectApi,
    private readonly editField: EditFieldView,
    private readonly buffer: BufferView,
    private readonly announcer: AnnouncerView,
    private readonly beep: BeepView,
    private readonly window: WindowView,
    // **Optional, and its absence is an answer rather than a gap** (spec B9, decision 3):
    // a window with nothing that can ask a person refuses a host key rather than trusting
    // one because nobody was there to object. Every far end except SSH asks nothing, so
    // this is only supplied where the dialogs are.
    private readonly questions?: QuestionView,
  ) {}

  /**
   * What the window opens with: the session the launch brought, or nothing.
   *
   * **Nothing is the ordinary case since B7** — an empty window is a state with
   * obligations, and the first of them is that it says so rather than looking like a
   * session that has gone quiet.
   */
  async start(): Promise<void> {
    await this.show(await this.connect.connected());
  }

  /** What this machine offers, for whoever is rendering the connect list. */
  connectable(): Promise<Connectable[]> {
    return this.connect.connectable();
  }

  /**
   * Connect to one profile, replacing whatever was running.
   *
   * **A failure is spoken and nothing else happens**: the session that was running is
   * still running and still attached, so what the user loses by choosing something that
   * would not start is nothing at all.
   *
   * Answers whether the window is on the new far end now, because the caller has something
   * to decide with it: the connect dialog closes on success and stays open on failure, so
   * the user is left somewhere they can choose again (spec A8, decision 4).
   */
  async connectTo(id: ProfileId): Promise<boolean> {
    let connected: Connected;
    try {
      connected = await this.connect.use(id, {
        // **Said while it happens, because a listener with no feedback cannot tell a slow
        // network from a dead one** (spec B9, decision 6). These are the backend's own
        // sentences: only it knows which stage a connection has reached.
        onProgress: (said) => this.announcer.announce(said),
        onQuestion: (question) =>
          this.questions === undefined
            ? Promise.resolve({ answer: 'GiveUp' as const })
            : this.questions.ask(question),
      });
    } catch (why) {
      // The sentence is the backend's, because only the backend knows what went wrong.
      // Every other announced string in this file is pinned here; this one is the
      // exception and the reason is that a pinned string could only say "it failed".
      this.announcer.announce(reason(why));
      return false;
    }
    await this.show(connected);
    this.announcer.announce(connectedMessage(connected.label, connected.note));
    return true;
  }

  /**
   * Point the window at a connection, or at nothing.
   *
   * **The buffer is cleared here, between the two calls**, which is the whole reason
   * attaching is a separate call from using a profile (spec B7, decision 1): a buffer
   * still holding one shell's output while another's arrives under it is a transcript of
   * a session that never happened.
   */
  private async show(connected: Connected | null): Promise<void> {
    // Set here rather than beside the announcement, because a launch that brought a session
    // reaches this without going through `connectTo` at all.
    this.noteSaidIntegrationIsMissing =
      connected?.note?.includes('shell integration') ?? false;
    this.buffer.clear();
    this.openBlocks.clear();
    this.tooBig.clear();
    this.echoed.clear();
    this.session = connected === null ? null : connected.session;
    // The terminal window belongs to a session: with none there is a Connect button and
    // nothing to type into (spec A10). Done before the attach, so nothing the new session
    // says arrives at a window still showing the last one's shape.
    this.window.showTerminal(connected !== null);

    if (connected === null) {
      this.window.connectedTo(null);
      this.window.status(disconnectedStatus);
      this.announcer.announce(notConnectedMessage);
      return;
    }
    this.window.connectedTo(connected.label);
    // The status is left to `ConnectionChanged`, which the session emits and holds until
    // this attach collects it — so "connecting" and "connected" have one producer rather
    // than two that could disagree about which one this window is in.
    await this.backend.attachSession(connected.session, (event) => {
      this.handleEvent(event);
    });
  }

  async submit(): Promise<void> {
    const text = this.editField.value().trim();
    // **Unreachable from the window since A10**, which took the edit field away when there
    // is no session — and kept as the guard it always was, for the submission that races a
    // session ending. Answered rather than swallowed, and the text is kept: silence is what
    // a shell that is thinking sounds like.
    if (this.session === null) {
      this.announcer.announce(notConnectedMessage);
      return;
    }
    // An empty field is submitted like any other line (spec B4.9). It used to return
    // here, so nothing was written, the shell never redrew its prompt and the user heard
    // nothing at all — and a blank line is ordinary input to a running program besides,
    // a REPL or a "press Enter to continue". What it does not do is open a block: an
    // empty submission matches no echo, so a bare Enter is a re-orient gesture rather
    // than a command, and the prompt it brings back is the answer to it.
    const ack = await this.backend.submitCommand(this.session, text);
    // The session went away between the keypress and the invoke — replaced, or ended. The
    // same answer as the check above, for the same reason: the line was not run anywhere,
    // so the text stays where the user can send it again.
    if (ack.status === 'NotConnected') {
      this.announcer.announce(notConnectedMessage);
      return;
    }
    if (text === '') {
      this.editField.clear();
      return;
    }
    // The block appears immediately, tagged with the id from the ack (ARCHITECTURE
    // round-trip); later events append under it.
    //
    // Set the command line here too: an event (CommandStarted/Output) can arrive over
    // the Channel before this ack resolves and open the block with an empty heading, so
    // openBlock updates it rather than being gated out.
    //
    // Unless the shell has already said what this block is running. That is the one
    // heading the frontend must not overwrite: this text is what the user typed, which
    // is only a guess about which block will run it, and the whole of B6.1 is that a
    // drifted id must not be able to put the wrong words on a block.
    this.buffer.openBlock(
      ack.command_id,
      this.echoed.has(ack.command_id) ? '' : text,
    );
    this.openBlocks.add(ack.command_id);
    this.editField.clear();
  }

  // Ensure a block exists for an event's command id, opening one with an empty heading
  // if the submit ack has not arrived yet (a scripting race). Never overwrites a line
  // already set: openBlock ignores an empty line for an existing block.
  // Put the shell's own words on the block: `command_line` is read from the echo the
  // shell wrote for this block, so it says what is running rather than what was typed
  // (spec B6.1, decision 1). `null` means the shell did not say — an unintegrated
  // session, or an echo the backend would not guess at — and the heading the submit ack
  // gave the block stands.
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
        // A submit already opened the block; a started event with no block (e.g. a
        // scripting race) opens one lazily with an empty heading.
        this.ensureBlock(event.command_id);
        this.headByTheEcho(event.command_id, event.command_line);
        break;
      case 'Output':
        this.ensureBlock(event.command_id);
        // Rendering only, and nothing is said here: since A6 this event carries no
        // verdict, so whether any of this text is spoken arrives as its own `Announce`.
        // The render-before-announce invariant (spec A5.2) still holds and is now the
        // backend's to keep — it emits this event before the `Announce` about it, and
        // the per-session channel delivers in order, so the text is in the buffer before
        // anything speaks it.
        this.buffer.appendOutput(event.command_id, event.text);
        break;
      case 'CommandFinished':
        this.ensureBlock(event.command_id);
        // Nothing is said here about the exit code, and since A6 the event carries no
        // code to say. A failure arrives as `Announce { Failed }` after this one —
        // after the remainder of the output has been read, which is the order a
        // listener needs: the error text first, the verdict about it second.
        // Announcing here as well (an A3-era leftover, found in B6's manual pass) said
        // it twice and said it first, ahead of the line it was about.
        //
        // Beep if this command ever carried a too-big verdict: "you were told it is
        // too big; the beep tells you it is done." That verdict now reaches us only
        // through `Announce { TooBig }`, which is what arms `tooBig`. A fully auto-read
        // success gets no extra finish speech — its output was already read.
        if (this.tooBig.has(event.command_id)) {
          this.beep.beep();
        }
        this.tooBig.delete(event.command_id);
        this.openBlocks.delete(event.command_id);
        this.echoed.delete(event.command_id);
        break;
      case 'PromptDrawn':
        // Rendered first and spoken second, which is the render-before-announce invariant
        // A5.2 pinned: the text is in the buffer before anything says it, so a listener who
        // reaches for it after hearing it finds it there.
        //
        // Spoken every time rather than only when it changes (spec B5.6, decision 3): the
        // prompt is how a user knows where they are, and a terminal repeats it after every
        // command. If the repetition proves tiring in use it becomes a setting, not a
        // silent default.
        this.buffer.appendPrompt(event.text);
        this.announcer.announce(event.text);
        break;
      case 'CommandInterrupted':
        // Terminal, like CommandFinished, and silent like it too: the block bookkeeping
        // happens and nothing is announced (B4.1). What the user hears is the shell's own
        // prompt coming back, read as ordinary output — evidence from the far end rather
        // than a claim Acter makes about a signal it only asked for. No beep either: the
        // beep answers "the too-big output you were warned about has finished", and a
        // command that was stopped never finished.
        this.ensureBlock(event.command_id);
        this.tooBig.delete(event.command_id);
        this.openBlocks.delete(event.command_id);
        this.echoed.delete(event.command_id);
        break;
      case 'IntegrationUnavailable':
        // Session-scoped, like the alt-screen pair: it carries no command id because it
        // fires before any command exists.
        //
        // **Said once per session, and not at all when the connection already said it.**
        // For an SSH far end this event is certain rather than diagnostic — the session is
        // unintegrated by construction (spec B9, decision 2) — and the connection announced
        // it with the far end's name attached, which is strictly more use.
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
        // Speaking is its own event (B1.5): the buffer is loaded by Output, and this
        // says something about text that is already there. Nothing is appended here.
        this.handleAnnouncement(event.command_id, event.announcement);
        break;
      case 'ConnectionChanged':
        this.connectionChanged(event.state);
        break;
      case 'TitleChanged':
        // No UX decided yet (no producers in Phase 1); handled to keep the switch
        // exhaustive.
        break;
      default:
        assertNever(event);
    }
  }

  // One announcement, turned into one of the pinned strings this module owns. The
  // backend sends what happened, never the words: `TooBig` carries a line count rather
  // than the text, because past the threshold the text is not held backend-side at all.
  /**
   * What the window says about its connection: the status region always, and the titles
   * when a connection came or went.
   *
   * The far end's *name* is not on this event and deliberately so — `ConnectionState` is
   * about the transport, and which shell is behind it is the session's business. Until B7
   * makes that a question anyone can ask, the window names what the app was started with,
   * which the backend reports once at startup.
   */
  private connectionChanged(state: ConnectionState): void {
    switch (state) {
      case 'Connecting':
        this.window.status(connectingStatus);
        break;
      case 'Connected':
        this.window.status(connectedStatus);
        break;
      case 'Reconnecting':
        // No producer yet: a transport that can reconnect is SSH's (spec A9, decision 5).
        this.window.status(connectingStatus);
        break;
      case 'Disconnected':
        this.window.status(disconnectedStatus);
        // Nothing is behind the window any more, so it stops claiming to be anything.
        this.window.connectedTo(null);
        // **The buffer stays and the edit field goes** (spec A10). What is left is the
        // record of a session that ended, which a user who typed `exit` by accident must
        // not lose; what has no purpose left is a field with nothing to submit to.
        this.session = null;
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
    if (this.editField.isFocused()) {
      this.buffer.focus();
    } else {
      this.editField.focus();
    }
  }

  escapeToEditField(): void {
    if (this.buffer.containsFocus()) {
      this.editField.focus();
    }
  }

  /**
   * Report a keystroke the frontend chose not to handle, and say what came back.
   *
   * The key travels, never the meaning: the binding table is the domain's, so this
   * method knows nothing about interrupting and needs no change when a second binding
   * arrives (spec A3.2 decision 1).
   */
  async reportKey(press: KeyPress): Promise<void> {
    // Nothing to report a keystroke to. `NothingToActOn`'s words are what a window with no
    // session would have said anyway — there is nothing running to stop — and saying them
    // without a round trip keeps the answer immediate.
    if (this.session === null) {
      this.announcer.announce(nothingToStopMessage);
      return;
    }
    const ack = await this.backend.sendKey(this.session, press);
    switch (ack) {
      case 'Applied':
        // Nothing: the session speaks for itself when the intent lands.
        break;
      case 'NothingToActOn':
        this.announcer.announce(nothingToStopMessage);
        break;
      case 'Unbound':
        this.announcer.announce(unboundKeyMessage);
        break;
      default:
        assertNeverAck(ack);
    }
  }

  /**
   * Whether the edit field is holding a selection — the question that decides whether
   * the platform still owns a keystroke rather than the session.
   *
   * Only the edit field is asked, because only the edit field reports keystrokes at all
   * (DESIGN's layer 2: the interrupt belongs there and nowhere else). It is asked here
   * rather than read off the document because an input's selection is not the
   * document's — `window.getSelection()` cannot see one — which is why this is a view
   * question rather than a DOM one.
   */
  editFieldHasSelection(): boolean {
    return this.editField.hasSelection();
  }
}
