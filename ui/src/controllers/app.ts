// Role: controller — translates UI intents into backend calls and renders the
// backend's SessionEvent stream into the buffer, live region, and beep. Framework-free:
// sees only the BackendApi port and the view ports.

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

// Pinned announcement strings (spec decision 3). Every announced string is a domain
// requirement; this module is their single source in the frontend. The dynamic ones
// (N, exit code) are functions so the wording lives in exactly one place.
export const patienceMessage =
  'long command running, output is accumulating in the buffer';
export const altScreenEnteredMessage =
  'this program needs interactive mode, which is not available yet. Press Ctrl+C to return to the prompt';
export const altScreenLeftMessage = 'interactive program ended';
// This session's shell never announced itself, so Acter is not told how a command ended:
// no exit code and no verdict. Everything else a listener meets is unchanged — output is
// read aloud, it accumulates in the buffer, and a long command is still announced as
// running.
//
// **This sentence said the opposite for five entries** (spec A13). It claimed output would
// not be read automatically, which B4.4 stopped being true — "only the silence goes" — and
// which `policies/autoread.rs` has never consulted `Integration` about. B6.2 made the lie
// audible rather than causing it, by reading the prompt too, so a listener heard the claim
// and then immediately heard output.
//
// **And the wording is a user's, not this project's.** Offered three accurate corrections
// that all said "shell integration", "verdict" or "exit code", the user answered that not
// even they could understand them. So the sentence names what still works, names the one
// thing that does not in the words a person uses about a command, and sends anybody who
// wants the reason to a place they can read at their own pace — which is what F1 now opens
// (DESIGN's reliability case 2, backend event added in B6).
export const integrationUnavailableMessage =
  'You will hear what commands print here, but not whether they worked. Press F1 for help.';
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
export const disconnectedStatus = 'not connected';

/**
 * What this window is connected to, as **one string held in one place**.
 *
 * **A9 decision 2's three words are reversed here, 2026-08-27**, and the reason is the
 * user's: "no two truth places." The status region said "connected" while the connection
 * was announced as "connected to SSH: acter at 127.0.0.1, port 2222, bash, with no shell
 * integration set up on this host" — two descriptions of one fact, only one of which
 * survived being spoken once. Somebody who came back to the window an hour later could
 * find out *what* they were connected to from the title, and nothing at all about what
 * kind of session it was.
 *
 * So the region carries the whole sentence and the announcement *is* that string. There is
 * nothing to keep in step, because there is only one of it.
 *
 * A9's reasoning for a short label still holds for the two states with nothing to add:
 * "connecting" and "not connected" say everything there is.
 */
export function connectedStatus(label: string, note?: string | null): string {
  const said = `connected to ${label}`;
  return note === undefined || note === null ? said : `${said}, ${note}`;
}

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
  return connectedStatus(label, note);
}

// Unreachable while Ctrl+C is both the only key reported and the only key bound. It is
// still spoken, because the first thing a second reported key must not do is vanish.
export const unboundKeyMessage = 'that key does nothing here';

// **What handing the keyboard to the far end costs and buys, said once rather than left to
// be discovered** (spec 28, decision 1). Neither sentence names a mode: what changes for the
// person listening is whose history and whose completion they get, and "far-end-line mode"
// tells them nothing about that.
//
// It is the frontend's string, pinned here beside every other announced one, for the reason
// they all are: the backend sends what happened and never the words.
export const farEndLineOnMessage =
  'The program gets your keys now. Its own history and completion — Acter\u2019s are off.';
export const farEndLineOffMessage =
  'Acter gets your keys again. History and completion are back.';

// The two answers to `Ctrl+D` that only the frontend can voice, and the one it must not
// (spec 28, decision 9; roadmap 23.5).
//
// **The third case has no string on purpose**: when the far end takes the key and the
// session ends, the connection sentence already says so, and a second announcement would be
// Acter talking over the answer the user actually wanted.
//
// `Unsupported` is a shell nobody has measured an end-of-input answer for. It says what to
// do instead, because a listener who hears only that the key did nothing has no next step —
// which is the failure A3.1 named for "nothing running to stop".
export const noEndOfInputKeyMessage =
  'This shell has no key for end of input. Type exit and press Enter.';
// And `NothingToActOn` for that key means the session is not there any more, which is a
// different thing from a shell that has no answer.
export const sessionAlreadyEndedMessage = 'This session has already ended.';

/**
 * What to say about the answer to one keystroke, or `null` when nothing is to be said.
 *
 * **The frontend words the answer to the key it sent, and still does not decide what the key
 * means** (spec 28, decision 9). "Bound, and this far end has no measured answer" and
 * "bound, and nothing is listening" are different things to tell a listener, and for
 * `Ctrl+D` they are two different sentences — but which of them applies is the backend's
 * `KeyAck`, and the binding table stays behind the port.
 */
function answerTo(press: KeyPress, ack: KeyAck): string | null {
  const endOfInput =
    typeof press.key === 'object' &&
    press.key.Char === 'd' &&
    press.ctrl &&
    !press.shift &&
    !press.alt;
  switch (ack) {
    case 'Applied':
      // The far end has the key, and whatever it does about it is a better answer than
      // anything Acter could say — a session that ends says so through its connection.
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
  // What this window is connected to, kept so the status region can restate it without a
  // second, shorter description of the same fact being invented somewhere else.
  private connection: Connected | null = null;
  // Who owns the line being edited (spec 28, decision 1).
  //
  // **The backend holds the state that decides anything** — which bytes a key becomes,
  // whether Enter opens a block, which row goes in front of the listener — and this copy
  // decides only what this window shows and where focus goes. Keeping the deciding half here
  // would put a second binding table in the frontend, which is the seam B6 decision 4 exists
  // to prevent.
  private lineOwner: LineOwner = 'Local';

  constructor(
    private readonly backend: BackendApi,
    private readonly connect: ConnectApi,
    private readonly editField: EditFieldView,
    private readonly buffer: BufferView,
    // **Optional, because a window can exist without one** — the connect dialogs' own
    // harness builds a controller with the views it needs and no more, and a caller without
    // this one simply never leaves local-line mode.
    private readonly farEndField: FarEndFieldView | undefined,
    private readonly announcer: AnnouncerView,
    private readonly beep: BeepView,
    private readonly window: WindowView,
    // **Optional, and its absence is an answer rather than a gap** (spec B9, decision 3):
    // a window with nothing that can ask a person refuses a host key rather than trusting
    // one because nobody was there to object. Every far end except SSH asks nothing, so
    // this is only supplied where the dialogs are.
    private readonly questions?: QuestionView,
    // **A failure is acknowledged, not announced** (reported 2026-08-26). Optional for the
    // same reason `questions` is: a caller without one still gets the sentence, said the
    // old way, rather than losing it.
    private readonly failure?: MessageView,
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
  async connectTo(id: ProfileId, setUp: SetUp = 'Yes'): Promise<boolean> {
    let connected: Connected;
    try {
      connected = await this.connect.use(id, setUp, {
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

  /**
   * Say what this window is connected to.
   *
   * **Separate from `connectTo` since 13.3**, so it can be said once the connect dialogs
   * have closed and focus has landed in the edit field. Announced any earlier it went into a
   * live region that was about to be taken away, or into one that had just come back, and
   * either way a listener heard the shell's prompt and never what they had connected to.
   *
   * The words stay here with every other announced string; what moved is the moment. Answers
   * whether there was anything to say.
   */
  announceConnection(): boolean {
    if (this.connection === null) {
      return false;
    }
    this.announcer.announce(
      connectedMessage(this.connection.label, this.connection.note),
    );
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
    // **Read off the connection rather than out of its sentence** (spec B9.5, decision 13).
    // This used to search the note for the words "shell integration", which is exactly the
    // vocabulary A13 removed and B9.5 rewrote — so a reworded sentence silently changed what
    // a listener heard afterwards. The backend computes it beside the sentence now.
    this.noteSaidIntegrationIsMissing = connected?.limit_explained ?? false;
    // A new far end owns nothing until the user says so. The state is per session, like
    // everything else this method resets: a mode carried across a connection would change
    // what a key does in a shell the user never chose it for.
    this.setLineOwner('Local', false);
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
      this.connection = null;
      this.window.connectedTo(null);
      this.window.status(disconnectedStatus);
      this.announcer.announce(notConnectedMessage);
      return;
    }
    this.window.connectedTo(connected.label);
    // **The region carries the whole sentence, and the announcement is that same string.**
    // `ConnectionChanged` still owns the *transitional* states, which are the session's to
    // report; what it cannot know is the far end's name or what was learned about it while
    // connecting, so the connection itself is what says this.
    this.connection = connected;
    this.window.status(connectedStatus(connected.label, connected.note));
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
        //
        // **Applied to the line it names since 28** (decision 8): the far end writes its own
        // record, revisions and blanks included, and the buffer keeps that and nothing else.
        this.buffer.applyLine(
          event.command_id,
          event.line,
          event.revision,
          event.text,
        );
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
      case 'FarEndLine':
        // **Nothing is announced here, and that is the decision rather than an omission**
        // (spec 28, decision 3). The element is an ARIA text box holding the far end's row
        // and its caret, so NVDA answers every key out of its own text-box behaviour: the
        // row when the row changed, the character at the caret when only the cursor moved,
        // the character typed when the user typed, and "blank" for a row a key emptied.
        // Announcing as well would say it twice — measured: a live region answering
        // alongside produced two utterances in the same millisecond.
        this.farEndField?.render(event.text, event.caret);
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
        // The same sentence the connection produced, not a shorter one that would make the
        // region disagree with what was said a moment ago.
        if (this.connection !== null) {
          this.window.status(
            connectedStatus(this.connection.label, this.connection.note),
          );
        }
        break;
      case 'Reconnecting':
        // No producer yet: a transport that can reconnect is SSH's (spec A9, decision 5).
        this.window.status(connectingStatus);
        break;
      case 'Disconnected':
        this.window.status(disconnectedStatus);
        // Nothing is behind the window any more, so it stops claiming to be anything.
        this.connection = null;
        this.window.connectedTo(null);
        // **The buffer stays and the edit field goes** (spec A10). What is left is the
        // record of a session that ended, which a user who typed `exit` by accident must
        // not lose; what has no purpose left is a field with nothing to submit to.
        this.session = null;
        // The far end that owned the line is gone, so the line comes back to Acter — which
        // is not a mode change the user made and therefore not one they are told about.
        // What they are told is that the session ended.
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
      // `NothingToActOn` is what a session that has gone would have answered, so the words
      // are the same ones — which for `Ctrl+D` is "this session has already ended" and for
      // everything else is "nothing running to stop". Neither can be null: `Applied` is the
      // only silent ack, and nothing was applied.
      this.announcer.announce(
        answerTo(press, 'NothingToActOn') ?? nothingToStopMessage,
      );
      return;
    }
    const ack = await this.backend.sendKey(this.session, press);
    // 'Applied' answers nothing: the session speaks for itself when the intent lands.
    const said = answerTo(press, ack);
    if (said !== null) {
      this.announcer.announce(said);
    }
  }

  /**
   * Hand the keyboard to the far end, or take it back (spec 28, decision 1).
   *
   * **The announcement says what the user gains and loses rather than naming a mode**, and
   * focus moves with it: the line's owner and the element the keys go to are the same fact
   * said twice, so leaving focus behind would put a listener in a field whose keys no longer
   * do what it says they do.
   */
  async toggleLineOwner(): Promise<void> {
    if (this.session === null || this.farEndField === undefined) {
      // Nothing to hand the keyboard to. The same answer the window gives every other
      // gesture it cannot act on, in the same words it has used since it opened.
      this.announcer.announce(notConnectedMessage);
      return;
    }
    const next: LineOwner = this.lineOwner === 'Local' ? 'FarEnd' : 'Local';
    await this.backend.setLineOwner(this.session, next);
    this.setLineOwner(next, true);
  }

  /** Whether the far end owns the line, which decides what Escape means. */
  farEndOwnsTheLine(): boolean {
    return this.lineOwner === 'FarEnd';
  }

  /**
   * Paste into the far end's own line editor (spec 28, decision 10).
   *
   * One invoke rather than a run of keystrokes, because only the backend knows whether the
   * far end asked for bracketed paste — and sending the wrapper to one that did not puts its
   * bytes into the line, while never sending it runs each pasted line as it arrives.
   */
  async pasteToFarEnd(text: string): Promise<void> {
    if (this.session === null || this.lineOwner !== 'FarEnd') {
      return;
    }
    await this.backend.paste(this.session, text);
  }

  /**
   * Show the window whichever line is being edited, and say which.
   *
   * `announce` is false when nothing changed for the user to hear about — a new connection
   * starting in local-line mode, which is where every session starts.
   */
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
