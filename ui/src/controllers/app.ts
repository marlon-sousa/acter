// Role: controller — translates UI intents into backend calls and renders the
// backend's SessionEvent stream into the buffer, live region, and beep. Framework-free:
// sees only the BackendApi port and the view ports.

import type {
  Announcement,
  CommandId,
  KeyPress,
  SessionEvent,
} from '../protocol';
import type { AnnouncerView } from '../ports/announcer_view';
import type { BackendApi } from '../ports/backend_api';
import type { BeepView } from '../ports/beep_view';
import type { BufferView } from '../ports/buffer_view';
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

export class AppController {
  // Commands with an open buffer block, so an event can find (or lazily open) one.
  private readonly openBlocks = new Set<CommandId>();
  // Commands that have carried a too-big chunk, so the completion beep fires on their
  // finish. View state the frontend legitimately owns (decision 2).
  private readonly tooBig = new Set<CommandId>();
  // Commands whose heading came from the shell's own echo, so the optimistic heading
  // from a submit ack never overwrites it (spec B6.1, decision 1).
  private readonly echoed = new Set<CommandId>();

  constructor(
    private readonly backend: BackendApi,
    private readonly editField: EditFieldView,
    private readonly buffer: BufferView,
    private readonly announcer: AnnouncerView,
    private readonly beep: BeepView,
  ) {}

  /** Attach to the session at startup; every SessionEvent flows to handleEvent. */
  async attach(): Promise<void> {
    await this.backend.attachSession((event) => this.handleEvent(event));
  }

  async submit(): Promise<void> {
    const text = this.editField.value().trim();
    // An empty field is submitted like any other line (spec B4.9). It used to return
    // here, so nothing was written, the shell never redrew its prompt and the user heard
    // nothing at all — and a blank line is ordinary input to a running program besides,
    // a REPL or a "press Enter to continue". What it does not do is open a block: an
    // empty submission matches no echo, so a bare Enter is a re-orient gesture rather
    // than a command, and the prompt it brings back is the answer to it.
    const ack = await this.backend.submitCommand(text);
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
        this.announcer.announce(integrationUnavailableMessage);
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
      case 'TitleChanged':
      case 'ConnectionChanged':
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
    const ack = await this.backend.sendKey(press);
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
