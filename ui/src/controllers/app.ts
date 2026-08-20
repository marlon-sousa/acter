// Role: controller — translates UI intents into backend calls and renders the
// backend's SessionEvent stream into the buffer, live region, and beep. Framework-free:
// sees only the BackendApi port and the view ports.

import type { Announcement, CommandId, SessionEvent } from '../protocol';
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
export const commandStoppedMessage = 'command stopped';
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

function assertNever(event: never): never {
  throw new Error(`unhandled SessionEvent variant: ${JSON.stringify(event)}`);
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
    if (text === '') {
      return;
    }
    // The block appears immediately, tagged with the id from the ack (ARCHITECTURE
    // round-trip); later events append under it.
    const ack = await this.backend.submitCommand(text);
    // Always set the command line authoritatively: an event (CommandStarted/Output)
    // can arrive over the Channel before this ack resolves and open the block with an
    // empty heading, so openBlock updates it here rather than being gated out.
    this.buffer.openBlock(ack.command_id, text);
    this.openBlocks.add(ack.command_id);
    this.editField.clear();
  }

  // Ensure a block exists for an event's command id, opening one with an empty heading
  // if the submit ack has not arrived yet (a scripting race). Never overwrites a line
  // already set: openBlock ignores an empty line for an existing block.
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
        break;
      case 'Output':
        this.ensureBlock(event.command_id);
        // Render-before-announce invariant (pinned by spec A5.2): append to the buffer
        // BEFORE announcing. The announcer's deferred drain guarantees the live region
        // mutates after this synchronous append, so spoken text is always already in the
        // buffer.
        this.buffer.appendOutput(event.command_id, event.text);
        if (event.read_mode === 'Auto') {
          this.announcer.announce(event.text);
        } else if (event.read_mode === 'TooBig') {
          this.tooBig.add(event.command_id);
          this.announcer.announce(tooBigMessage(lineCount(event.text)));
        }
        // Quiet: appended to the buffer, no speech.
        break;
      case 'CommandFinished':
        this.ensureBlock(event.command_id);
        if (event.read_mode === 'TooBig') {
          this.tooBig.add(event.command_id);
        }
        // Failure is always spoken, regardless of verdicts.
        if (event.exit_code !== 0) {
          this.announcer.announce(failureMessage(event.exit_code));
        }
        // Beep if this command ever carried a too-big verdict: "you were told it is
        // too big; the beep tells you it is done." A fully auto-read success gets no
        // extra finish speech — its output was already read.
        if (this.tooBig.has(event.command_id)) {
          this.beep.beep();
        }
        this.tooBig.delete(event.command_id);
        this.openBlocks.delete(event.command_id);
        break;
      case 'CommandInterrupted':
        // Terminal, like CommandFinished: same cleanup, but deliberately no beep. The
        // beep answers "the too-big output you were warned about has finished"; a
        // stopped command already has a spoken answer (A3.1 decision 7).
        this.ensureBlock(event.command_id);
        this.announcer.announce(commandStoppedMessage);
        this.tooBig.delete(event.command_id);
        this.openBlocks.delete(event.command_id);
        break;
      case 'CommandStillRunning':
        this.announcer.announce(patienceMessage);
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
}

// The chunk's line count, computed frontend-side for the too-big phrasing only — the
// verdict was already made backend-side (decision 2); this is not re-measuring.
function lineCount(text: string): number {
  return text.split('\n').length;
}
