// Role: test — controller behavior against fake backend, views, and beep. Covers
// every rendering rule in spec decision 2.

import { describe, expect, it } from 'vitest';

import type {
  CommandId,
  Connectable,
  Connected,
  KeyAck,
  KeyPress,
  LineId,
  LineOwner,
  LineRevision,
  ProfileId,
  SessionEvent,
  SessionId,
  SetUp,
  SubmitAck,
} from '../../src/protocol';
import type { AnnouncerView } from '../../src/ports/announcer_view';
import type { BackendApi } from '../../src/ports/backend_api';
import type {
  ConnectApi,
  ConnectListener,
} from '../../src/ports/connect_api';
import type { BeepView } from '../../src/ports/beep_view';
import type { BufferView } from '../../src/ports/buffer_view';
import type { WindowView } from '../../src/ports/window_view';
import type { EditFieldView } from '../../src/ports/edit_field_view';
import type { FarEndFieldView } from '../../src/ports/far_end_field_view';
import {
  AppController,
  altScreenEnteredMessage,
  connectedMessage,
  notConnectedMessage,
  integrationUnavailableMessage,
  altScreenLeftMessage,
  failureMessage,
  nothingToStopMessage,
  outputContinuesMessage,
  patienceMessage,
  tooBigMessage,
  unboundKeyMessage,
  farEndLineOnMessage,
  farEndLineOffMessage,
  noEndOfInputKeyMessage,
  sessionAlreadyEndedMessage,
} from '../../src/controllers/app';

class FakeBackend implements BackendApi {
  submitted: string[] = [];
  private nextId = 1;
  private onEvent: ((event: SessionEvent) => void) | undefined;

  /** Every session this backend was attached to, in order. */
  attachedTo: SessionId[] = [];
  attachSession(
    session: SessionId,
    onEvent: (event: SessionEvent) => void,
  ): Promise<void> {
    this.attachedTo.push(session);
    this.onEvent = onEvent;
    return Promise.resolve();
  }
  /** Hold acks back, so an event can be delivered while a submission is still in
   * flight — the race that decides which heading a block ends up with. */
  deferAcks = false;
  private held: Array<() => void> = [];
  /** The answer to the next submission, so a test can be the session that went away. */
  refuseSubmissions = false;
  /** Which session each submitted line named. */
  submittedTo: SessionId[] = [];
  submitCommand(session: SessionId, line: string): Promise<SubmitAck> {
    this.submitted.push(line);
    this.submittedTo.push(session);
    if (this.refuseSubmissions) {
      return Promise.resolve({ status: 'NotConnected' });
    }
    const ack: SubmitAck = { status: 'Accepted', command_id: this.nextId++ };
    if (!this.deferAcks) {
      return Promise.resolve(ack);
    }
    return new Promise((resolve) => {
      this.held.push(() => resolve(ack));
    });
  }
  releaseAcks(): void {
    for (const release of this.held) {
      release();
    }
    this.held = [];
  }
  /** What the next sendKey answers; the backend owns the binding table, so a test
   * chooses the answer rather than the meaning. */
  keyAck: KeyAck = 'Applied';
  keysSent: KeyPress[] = [];
  sendKey(_session: SessionId, key: KeyPress): Promise<KeyAck> {
    this.keysSent.push(key);
    return Promise.resolve(this.keyAck);
  }
  /** Every hand-over of the line, in order (spec 28, decision 1). */
  owners: LineOwner[] = [];
  setLineOwner(_session: SessionId, owner: LineOwner): Promise<void> {
    this.owners.push(owner);
    return Promise.resolve();
  }
  pasted: string[] = [];
  paste(_session: SessionId, text: string): Promise<void> {
    this.pasted.push(text);
    return Promise.resolve();
  }
  /** Push an event as the backend would over the Channel. */
  emit(event: SessionEvent): void {
    this.onEvent?.(event);
  }
}

class FakeEditField implements EditFieldView {
  text = '';
  focused = true;
  clearedCount = 0;
  value(): string {
    return this.text;
  }
  clear(): void {
    this.text = '';
    this.clearedCount += 1;
  }
  focus(): void {
    this.focused = true;
  }
  isFocused(): boolean {
    return this.focused;
  }
  selected = false;
  hasSelection(): boolean {
    return this.selected;
  }
}

class FakeBuffer implements BufferView {
  opened: Array<{ commandId: CommandId; commandLine: string }> = [];
  appended: Array<{
    commandId: CommandId;
    line: LineId;
    revision: LineRevision;
    text: string;
  }> = [];
  /** Prompts the shell drew, in the order the buffer was asked to keep them (B5.6). */
  prompts: string[] = [];
  focused = false;
  openBlock(commandId: CommandId, commandLine: string): void {
    this.opened.push({ commandId, commandLine });
  }
  applyLine(
    commandId: CommandId,
    line: LineId,
    revision: LineRevision,
    text: string,
  ): void {
    this.appended.push({ commandId, line, revision, text });
  }
  appendPrompt(text: string): void {
    this.prompts.push(text);
  }
  /** How many times the buffer was emptied, and what it forgot when it was. */
  cleared = 0;
  clear(): void {
    this.cleared += 1;
    this.opened = [];
    this.appended = [];
    this.prompts = [];
  }
  focus(): void {
    this.focused = true;
  }
  containsFocus(): boolean {
    return this.focused;
  }
}

class FakeAnnouncer implements AnnouncerView {
  announcements: string[] = [];
  /** How many times a caller said a dialog had closed (spec 13.3). */
  returns = 0;
  announce(text: string): void {
    this.announcements.push(text);
  }
  documentReturned(): void {
    this.returns += 1;
  }
}

class FakeBeep implements BeepView {
  beeps = 0;
  beep(): void {
    this.beeps += 1;
  }
}

/** What the window was told to call itself and to say, in order (spec A9). */
class FakeWindow implements WindowView {
  titles: Array<string | null> = [];
  statuses: string[] = [];
  /** Whether the terminal window was shown or taken away, in order (spec A10). */
  terminals: boolean[] = [];
  connectedTo(name: string | null): void {
    this.titles.push(name);
  }
  status(text: string): void {
    this.statuses.push(text);
  }
  showTerminal(live: boolean): void {
    this.terminals.push(live);
  }
  /** Whether the local command line was shown or taken away, in order (spec 28). */
  localLines: boolean[] = [];
  showLocalLine(showing: boolean): void {
    this.localLines.push(showing);
  }
}

/**
 * The far end's command line, as the controller drives it (spec 28, decision 2).
 *
 * It records rather than renders: what an ARIA text box does with a row and a caret is the
 * adapter's own test, and what this one asks is whether the controller hands it the right
 * ones and moves focus with the mode.
 */
class FakeFarEndField implements FarEndFieldView {
  rendered: Array<{ text: string | null; caret: number }> = [];
  showing = false;
  focused = false;
  render(text: string | null, caret: number): void {
    this.rendered.push({ text, caret });
  }
  show(showing: boolean): void {
    this.showing = showing;
  }
  focus(): void {
    this.focused = true;
  }
  isFocused(): boolean {
    return this.focused;
  }
}

/**
 * Connecting, as three answers a test chooses (spec B7).
 *
 * `atStartup` is the difference between the two windows B7 created: one that a launch
 * brought a session to, and one connected to nothing at all.
 */
class FakeConnect implements ConnectApi {
  rows: Connectable[] = [
    {
      id: { profile: 'Shell', kind: 'Cmd' },
      label: 'Command Prompt',
      available: true,
      instructions: null,
      variants: [],
    },
  ];
  atStartup: Connected | null = {
    session: 1,
    label: 'Command Prompt',
    note: null,
    limit_explained: false,
  };
  /** What the far end has to say about itself, once, at connection (spec B9). */
  note: string | null = null;
  /**
   * Whether that note already told the listener this session cannot say how a command went
   * (spec B9.5, decision 13).
   *
   * **A fact rather than a phrase to look for.** The controller used to search the note for
   * the words "shell integration" — the vocabulary A13 removed and B9.5 rewrote — so the
   * sentence and the suppression could not be changed independently.
   */
  limitExplained = false;
  /** What the Connect dialog's checkbox said on each attempt (spec B9.5, decision 9). */
  setUps: SetUp[] = [];
  /** What it says while it is connecting (spec B9, decision 6). */
  progress: string[] = [];
  /** The sentence `use` rejects with instead of connecting, when a test wants a failure. */
  refuses: string | null = null;
  used: ProfileId[] = [];
  private nextSession = 1;

  connectable(): Promise<Connectable[]> {
    return Promise.resolve(this.rows);
  }
  use(id: ProfileId, setUp: SetUp, listener?: ConnectListener): Promise<Connected> {
    this.used.push(id);
    this.setUps.push(setUp);
    for (const said of this.progress) {
      listener?.onProgress?.(said);
    }
    if (this.refuses !== null) {
      return Promise.reject(this.refuses);
    }
    this.nextSession += 1;
    return Promise.resolve({
      session: this.nextSession,
      label: id.profile === 'Distribution' ? `WSL: ${id.name}` : 'Command Prompt',
      note: this.note,
      limit_explained: this.limitExplained,
    });
  }
  connected(): Promise<Connected | null> {
    return Promise.resolve(this.atStartup);
  }
}

/**
 * A window that has already opened onto whatever `connect.atStartup` says.
 *
 * Started here rather than in each test because since B7 a controller with no session
 * refuses everything: "connected" is the precondition of nearly every rule in this file,
 * and the unconnected window has a describe block of its own.
 */
async function makeApp(connect: FakeConnect = new FakeConnect()) {
  const backend = new FakeBackend();
  const editField = new FakeEditField();
  const buffer = new FakeBuffer();
  const announcer = new FakeAnnouncer();
  const beep = new FakeBeep();
  const window = new FakeWindow();
  const farEndField = new FakeFarEndField();
  const controller = new AppController(
    backend,
    connect,
    editField,
    buffer,
    farEndField,
    announcer,
    beep,
    window,
  );
  await controller.start();
  // The startup announcement and title are not what any of the older tests are about, so
  // they start from where a user starts: a window that has said what it is.
  announcer.announcements = [];
  window.titles = [];
  window.statuses = [];
  window.terminals = [];
  window.localLines = [];
  return {
    backend,
    connect,
    editField,
    buffer,
    farEndField,
    announcer,
    beep,
    window,
    controller,
  };
}

describe('submit', () => {
  it('submits trimmed text, opens the block tagged with the ack id, clears the field', async () => {
    const { backend, editField, buffer, controller } = await makeApp();
    editField.text = '  small  ';

    await controller.submit();

    expect(backend.submitted).toEqual(['small']);
    expect(buffer.opened).toEqual([{ commandId: 1, commandLine: 'small' }]);
    expect(editField.clearedCount).toBe(1);
  });

  // Inverted in B4.9, not deleted: this used to assert that an empty or whitespace-only
  // field was dropped where it stood, which is why a bare Enter did nothing at all — no
  // bytes, so the shell never redrew its prompt and the user heard silence. It goes to
  // the far end now, and opens no block, because an empty submission matches no echo: a
  // bare Enter is a re-orient gesture rather than a command.
  it('submits empty and whitespace-only input, and opens no block for it', async () => {
    const { backend, buffer, editField, controller } = await makeApp();
    editField.text = '   ';

    await controller.submit();

    expect(backend.submitted).toEqual(['']);
    expect(buffer.opened).toEqual([]);
    expect(editField.clearedCount).toBe(1);
  });
});

describe('what the window says about its connection (spec A9)', () => {
  /** The state a user meets first, and the one that had no words: a window opening onto a
   * shell that takes seconds to start used to say nothing at all (roadmap 23.7). */
  it('says it is connecting', async () => {
    const { backend, window, controller } = await makeApp();

    backend.emit({ type: 'ConnectionChanged', state: 'Connecting' });

    expect(window.statuses).toContain('connecting');
  });

  /**
   * **The region says the whole thing, and the announcement is that same string** — A9
   * decision 2 reversed on use, 2026-08-27: "no two truth places". The region used to say
   * "connected" while the connection was announced with the far end's name and what was
   * learned about it, so the two descriptions of one fact could not be kept in step and
   * only one of them survived being spoken once.
   */
  it('says what it is connected to, in the region and in the announcement alike', async () => {
    const ubuntu: ProfileId = { profile: 'Distribution', name: 'Ubuntu' };
    const { connect, window, announcer, controller } = await makeApp();
    connect.note = 'bash, which Acter cannot set up yet.';

    await controller.connectTo(ubuntu);
    controller.announceConnection();

    const said = 'connected to WSL: Ubuntu, bash, which Acter cannot set up yet.';
    expect(window.statuses).toContain(said);
    expect(announcer.announcements).toContain(said);
  });

  /** And the session's own `Connected` restates it rather than shortening it. */
  it('restates the same sentence when the far end reports it is connected', async () => {
    const ubuntu: ProfileId = { profile: 'Distribution', name: 'Ubuntu' };
    const { backend, window, controller } = await makeApp();
    await controller.connectTo(ubuntu);
    window.statuses.length = 0;

    backend.emit({ type: 'ConnectionChanged', state: 'Connected' });

    expect(window.statuses).toContain('connected to WSL: Ubuntu');
  });

  /** A far end that goes away leaves the window saying so, and stops claiming to be
   * connected to something that is gone (spec A9, decision 4). */
  it('says it is not connected when the far end goes away, and drops the name', async () => {
    const { backend, window, controller } = await makeApp();

    backend.emit({ type: 'ConnectionChanged', state: 'Disconnected' });

    expect(window.statuses).toContain('not connected');
    expect(window.titles).toContain(null);
  });

  /** Reconnecting has no producer until SSH, but a listener meeting it should hear
   * something true rather than nothing. */
  it('treats reconnecting as connecting rather than falling silent', async () => {
    const { backend, window, controller } = await makeApp();

    backend.emit({ type: 'ConnectionChanged', state: 'Reconnecting' });

    expect(window.statuses).toContain('connecting');
  });
});

describe('the prompt a marked shell drew (spec B5.6)', () => {
  /** A shell that marks all four boundaries puts its prompt in the `A..B` region, which
   * block content excludes — so before this entry the working directory and the git branch
   * a listener steers by were audible nowhere at all. */
  it('speaks the prompt and keeps it in the buffer', async () => {
    const { backend, buffer, announcer, controller } = await makeApp();

    backend.emit({ type: 'PromptDrawn', text: 'C:\projects\acter (main)>' });

    expect(buffer.prompts).toEqual(['C:\projects\acter (main)>']);
    expect(announcer.announcements).toContain('C:\projects\acter (main)>');
  });

  /** Every time, not only when it changes: the same command run twice in two directories
   * has to say where each one ran (spec B5.6, decision 3). */
  it('speaks an unchanged prompt again rather than falling silent', async () => {
    const { backend, announcer, controller } = await makeApp();

    backend.emit({ type: 'PromptDrawn', text: 'acter>' });
    backend.emit({ type: 'PromptDrawn', text: 'acter>' });

    expect(announcer.announcements.filter((said) => said === 'acter>')).toHaveLength(2);
  });

  /** It opens nothing: a prompt is not a command, and a block opened for one would put an
   * empty heading into the sequence a listener walks with `h`. */
  it('opens no block', async () => {
    const { backend, buffer, controller } = await makeApp();

    backend.emit({ type: 'PromptDrawn', text: 'acter>' });

    expect(buffer.opened).toEqual([]);
  });
});

describe('the block heading (spec B6.1)', () => {
  it("heads the block with the command line the shell echoed", async () => {
    const { backend, buffer, controller } = await makeApp();

    backend.emit({
      type: 'CommandStarted',
      command_id: 1,
      command_line: 'git status',
    });

    expect(buffer.opened).toEqual([
      // Opened lazily with no heading, then headed by what the shell said it is
      // running. An id that drifted can no longer put the wrong words on a block.
      { commandId: 1, commandLine: '' },
      { commandId: 1, commandLine: 'git status' },
    ]);
  });

  it('leaves the heading alone when the shell did not say', async () => {
    const { backend, editField, buffer, controller } = await makeApp();
    editField.text = 'git status';

    await controller.submit();
    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });

    expect(buffer.opened).toEqual([{ commandId: 1, commandLine: 'git status' }]);
  });

  it('never lets a late submit ack overwrite a heading the shell gave', async () => {
    const { backend, editField, buffer, controller } = await makeApp();
    editField.text = 'what the user typed';
    backend.deferAcks = true;

    const submitting = controller.submit();
    backend.emit({
      type: 'CommandStarted',
      command_id: 1,
      command_line: 'what the shell read',
    });
    backend.releaseAcks();
    await submitting;

    expect(buffer.opened).toEqual([
      { commandId: 1, commandLine: '' },
      { commandId: 1, commandLine: 'what the shell read' },
      // The ack still opens its block — that is what makes one appear the instant Enter
      // is pressed — but with no heading of its own to impose: an empty line leaves an
      // existing block's heading exactly as it is.
      { commandId: 1, commandLine: '' },
    ]);
  });
});

describe('event rendering (decision 2)', () => {
  it('Output appends the text, and the ReadAloud about it speaks it', async () => {
    const { backend, buffer, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'hello from acter',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'hello from acter' },
    });

    expect(buffer.appended).toEqual([
      { commandId: 1, line: 1, revision: 'Appended', text: 'hello from acter' },
    ]);
    expect(announcer.announcements).toEqual(['hello from acter']);
  });

  it('renders an auto-read chunk into the buffer before announcing it (A5.2)', async () => {
    const backend = new FakeBackend();
    const order: string[] = [];
    // Two fakes sharing one call log: the invariant under test is cross-view ordering,
    // which the per-view recordings cannot show.
    const buffer: BufferView = {
      openBlock: () => {},
      applyLine: () => {
        order.push('buffer');
      },
      appendPrompt: () => {
        order.push('buffer');
      },
      clear: () => {},
      focus: () => {},
      containsFocus: () => false,
    };
    const announcer: AnnouncerView = {
      announce: () => {
        order.push('announce');
      },
      documentReturned: () => {},
    };
    const controller = new AppController(
      backend,
      new FakeConnect(),
      new FakeEditField(),
      buffer,
      new FakeFarEndField(),
      announcer,
      new FakeBeep(),
      new FakeWindow(),
    );
    await controller.start();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    // Two events now, in the order the backend sends them: A6 moved this invariant from
    // "append before announcing inside one handler" to "the render event before the
    // announce about it". The channel delivers in order, so obeying that order is enough.
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'hello',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'hello' },
    });

    expect(order).toEqual(['buffer', 'announce']);
  });

  it('a too-big chunk is appended whole and announced by its line count', async () => {
    const { backend, buffer, announcer, controller } = await makeApp();
    const text = Array.from({ length: 40 }, (_, i) => `line ${i + 1}`).join('\n');

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text,
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 40 },
    });

    expect(buffer.appended).toEqual([
      { commandId: 1, line: 1, revision: 'Appended', text },
    ]);
    expect(announcer.announcements).toEqual([tooBigMessage(40)]);
    expect(announcer.announcements[0]).toBe('40 lines arrived, too big to read');
  });

  it('an Output with no announcement after it appends and says nothing', async () => {
    const { backend, buffer, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'still working',
    });

    expect(buffer.appended).toEqual([{ commandId: 1, line: 1, revision: 'Appended', text: 'still working' }]);
    expect(announcer.announcements).toEqual([]);
  });

  it('a successful fully-auto-read command gets no extra finish speech and no beep', async () => {
    const { backend, announcer, beep, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'hello from acter',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'hello from acter' },
    });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    expect(announcer.announcements).toEqual(['hello from acter']);
    expect(beep.beeps).toBe(0);
  });

  // The event shape the backend actually produces: the text is rendered quietly, the
  // speech about it rides an `Announce`, and the failure verdict is a second `Announce`
  // sent after the terminal event. Written this way because B6's manual NVDA pass heard
  // the failure spoken TWICE and heard it FIRST — before the error line it was about —
  // which is what a second producer in `CommandFinished` did.
  it('announces the output and then the failure, once each, in that order', async () => {
    const { backend, announcer, beep, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'error: boom',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'error: boom' },
    });
    backend.emit({ type: 'CommandFinished', command_id: 1 });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'Failed', exit_code: 2 },
    });

    // The error text first, the verdict about it second, and each said exactly once.
    expect(announcer.announcements).toEqual(['error: boom', failureMessage(2)]);
    expect(beep.beeps).toBe(0);
  });

  // A finish carries no speech of its own: `CommandFinished` is a lifecycle event, and
  // everything spoken about a command comes from an `Announce`.
  it('a failing command that finishes says nothing until the Failed announcement', async () => {
    const { backend, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    expect(announcer.announcements).toEqual([]);
  });

  // B4.1: the stop is not Acter's to announce. What the user hears is the shell's own
  // prompt coming back, read as ordinary output — so this event does its bookkeeping and
  // says nothing at all.
  it('CommandInterrupted announces nothing and closes the block', async () => {
    const { backend, buffer, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'phase one',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'phase one' },
    });
    backend.emit({ type: 'CommandInterrupted', command_id: 1 });

    expect(announcer.announcements).toEqual(['phase one']);
    // The block was closed, so a later event for the same id opens a fresh one.
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'late',
    });
    expect(buffer.opened).toEqual([
      { commandId: 1, commandLine: '' },
      { commandId: 1, commandLine: '' },
    ]);
  });

  it('does not beep on a stopped command that had carried a too-big chunk', async () => {
    const { backend, beep, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'a\nb',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 2 },
    });
    backend.emit({ type: 'CommandInterrupted', command_id: 1 });

    // The beep answers "your too-big output finished", and a command that was stopped
    // never finished. Nothing is spoken either (B4.1): the too-big warning stands as the
    // last thing said, and the next thing the user hears is the shell.
    expect(beep.beeps).toBe(0);
    expect(announcer.announcements.at(-1)).toBe(tooBigMessage(2));
  });

  it('clears the too-big flag on interrupt, so a reused id does not beep later', async () => {
    const { backend, beep, controller } = await makeApp();

    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'a\nb',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 2 },
    });
    backend.emit({ type: 'CommandInterrupted', command_id: 1 });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    expect(beep.beeps).toBe(0);
  });

  it('beeps on finish when an earlier chunk carried a too-big verdict', async () => {
    const { backend, beep, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'a\nb',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 2 },
    });
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'trickle',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'trickle' },
    });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    expect(beep.beeps).toBe(1);
  });

  // The remainder flushed at the end of a command can itself be too big. That verdict
  // used to ride `CommandFinished.read_mode`; since A6 it is an `Announce` the backend
  // sends before closing the block, and it must still arm the beep.
  it('beeps when the verdict on the final remainder is too-big', async () => {
    const { backend, beep, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 900 },
    });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    expect(beep.beeps).toBe(1);
  });

  it('does not carry the too-big beep flag across commands', async () => {
    const { backend, beep, controller } = await makeApp();

    // Command 1 is too-big and beeps.
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'a\nb',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 2 },
    });
    backend.emit({ type: 'CommandFinished', command_id: 1 });
    // Command 2 is plain; it must not beep.
    backend.emit({
      type: 'Output',
      command_id: 2,
      line: 1,
      revision: 'Appended',
      text: 'ok',
    });
    backend.emit({
      type: 'Announce',
      command_id: 2,
      announcement: { kind: 'ReadAloud', text: 'ok' },
    });
    backend.emit({ type: 'CommandFinished', command_id: 2 });

    expect(beep.beeps).toBe(1);
  });

  it('AltScreenEntered and AltScreenLeft announce the pinned strings', async () => {
    const { backend, announcer, controller } = await makeApp();

    backend.emit({ type: 'AltScreenEntered' });
    backend.emit({ type: 'AltScreenLeft' });

    expect(announcer.announcements).toEqual([
      altScreenEnteredMessage,
      altScreenLeftMessage,
    ]);
    expect(announcer.announcements[0]).toBe(
      'this program needs interactive mode, which is not available yet. Press Ctrl+C to return to the prompt',
    );
    expect(announcer.announcements[1]).toBe('interactive program ended');
  });

  it('IntegrationUnavailable announces the pinned string and opens no block', async () => {
    const { backend, announcer, buffer, controller } = await makeApp();

    backend.emit({ type: 'IntegrationUnavailable' });

    expect(announcer.announcements).toEqual([integrationUnavailableMessage]);
    // **Spelled out rather than compared to the constant alone**, because the constant
    // was wrong for five entries and every test that only compared it to itself passed
    // the whole time (spec A13). What is pinned here is the sentence a person hears.
    expect(announcer.announcements[0]).toBe(
      'You will hear what commands print here, but not whether they worked. Press F1 for help.',
    );
    expect(buffer.opened).toEqual([]);
  });

  /** **The claim the old sentence made and the product had stopped honouring.** It said
   * output would not be read automatically; B4.4 reversed that ("only the silence goes")
   * and nothing updated the words. This asserts the behaviour the new sentence promises,
   * so the two can never drift apart again without a test saying so. */
  it('output in an unintegrated session is still read aloud', async () => {
    const { backend, announcer, controller } = await makeApp();

    backend.emit({ type: 'IntegrationUnavailable' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'acter>' },
    });

    expect(announcer.announcements).toContain('acter>');
  });

  it('TitleChanged and ConnectionChanged are silent no-ops', async () => {
    const { backend, announcer, buffer, beep, controller } = await makeApp();

    backend.emit({ type: 'TitleChanged', title: '~/acter' });
    backend.emit({ type: 'ConnectionChanged', state: 'Reconnecting' });

    expect(announcer.announcements).toEqual([]);
    expect(buffer.opened).toEqual([]);
    expect(beep.beeps).toBe(0);
  });

  it('lazily opens a block when an event arrives for an unsubmitted command', async () => {
    const { backend, buffer, controller } = await makeApp();

    // No submit happened; an Output races in first.
    backend.emit({
      type: 'Output',
      command_id: 7,
      line: 1,
      revision: 'Appended',
      text: 'orphan chunk',
    });

    expect(buffer.opened).toEqual([{ commandId: 7, commandLine: '' }]);
    expect(buffer.appended).toEqual([{ commandId: 7, line: 1, revision: 'Appended', text: 'orphan chunk' }]);
  });

  it('sets the command line on the ack even when an event opened the block first', async () => {
    // The scripting race: CommandStarted/Output for command 1 arrives over the Channel
    // before the submit ack resolves, lazily opening the block with an empty heading.
    const { backend, buffer, editField, controller } = await makeApp();
    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });

    editField.text = 'small';
    await controller.submit();

    // The block was opened empty by the event, then the ack authoritatively set 'small'.
    expect(buffer.opened).toEqual([
      { commandId: 1, commandLine: '' },
      { commandId: 1, commandLine: 'small' },
    ]);
  });

  it('does not reopen a block already opened by submit', async () => {
    const { backend, buffer, editField, controller } = await makeApp();
    editField.text = 'small';
    await controller.submit();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'hello from acter',
    });

    expect(buffer.opened).toEqual([{ commandId: 1, commandLine: 'small' }]);
  });
});

// **Far-end-line mode: the keyboard goes to the far end** (spec 28).
//
// What is asserted here is the controller's half: that the state reaches the backend, that
// focus and the two fields move with it, that the sentence says what is gained and lost, and
// that the far end's row is handed to the field rather than announced. What NVDA then says
// about that field is the measurement recorded in adapters/far_end_field.ts.
describe('handing the line to the far end (28)', () => {
  it('tells the backend, moves focus, and says what is gained and lost', async () => {
    const { backend, farEndField, window, announcer, controller } = await makeApp();

    await controller.toggleLineOwner();

    expect(backend.owners).toEqual(['FarEnd']);
    expect(farEndField.showing).toBe(true);
    expect(farEndField.focused).toBe(true);
    expect(window.localLines).toEqual([false]);
    expect(announcer.announcements).toEqual([farEndLineOnMessage]);
  });

  it('takes it back, and says that too', async () => {
    const { backend, editField, farEndField, window, announcer, controller } =
      await makeApp();
    await controller.toggleLineOwner();
    announcer.announcements = [];
    window.localLines = [];
    editField.focused = false;

    await controller.toggleLineOwner();

    expect(backend.owners).toEqual(['FarEnd', 'Local']);
    expect(farEndField.showing).toBe(false);
    expect(editField.focused).toBe(true);
    expect(window.localLines).toEqual([true]);
    expect(announcer.announcements).toEqual([farEndLineOffMessage]);
  });

  // Neither sentence names a mode. What changes for the person listening is whose history
  // and whose completion they get, and "far-end-line mode" says nothing about that.
  it('names what changes rather than naming a mode', () => {
    for (const said of [farEndLineOnMessage, farEndLineOffMessage]) {
      expect(said.toLowerCase()).not.toContain('mode');
      expect(said.toLowerCase()).toContain('history');
      expect(said.toLowerCase()).toContain('completion');
    }
  });

  it('says which way it went', () => {
    expect(farEndLineOnMessage).not.toBe(farEndLineOffMessage);
  });

  // Nothing to hand the keyboard to. The window answers in the words it has used since it
  // opened, rather than pretending the gesture worked.
  it('refuses in a window with no session, in the words that window already uses', async () => {
    const connect = new FakeConnect();
    connect.atStartup = null;
    const { announcer, controller, backend } = await makeApp(connect);

    await controller.toggleLineOwner();

    expect(backend.owners).toEqual([]);
    expect(announcer.announcements).toEqual([notConnectedMessage]);
  });

  // A mode carried across a connection would change what a key does in a shell the user
  // never chose it for, which is why the state is per session.
  it('comes back to Acter when the far end goes away, silently', async () => {
    const { farEndField, announcer, backend, controller } = await makeApp();
    await controller.toggleLineOwner();
    announcer.announcements = [];

    backend.emit({ type: 'ConnectionChanged', state: 'Disconnected' });

    expect(farEndField.showing).toBe(false);
    // The listener is told the session ended, and not also told about a mode change they
    // did not make.
    expect(announcer.announcements).toEqual([notConnectedMessage]);
  });

  // **Nothing is announced about the far end's row** (spec 28, decision 3), because the
  // element holding it is a text box and the reader speaks it. Announcing as well would say
  // it twice: measured on the bridge, a live region answering alongside produced two
  // utterances in the same millisecond, the reader's first.
  it('hands the row and the caret to the field and announces neither', async () => {
    const { backend, farEndField, announcer, controller } = await makeApp();
    await controller.toggleLineOwner();
    announcer.announcements = [];

    backend.emit({ type: 'FarEndLine', text: 'cargo test --all', caret: 16 });
    backend.emit({ type: 'FarEndLine', text: null, caret: 3 });

    expect(farEndField.rendered).toEqual([
      { text: 'cargo test --all', caret: 16 },
      { text: null, caret: 3 },
    ]);
    expect(announcer.announcements).toEqual([]);
  });

  it('pastes into the far end only while the far end owns the line', async () => {
    const { backend, controller } = await makeApp();

    await controller.pasteToFarEnd('ignored');
    expect(backend.pasted).toEqual([]);

    await controller.toggleLineOwner();
    await controller.pasteToFarEnd('cargo test --all');

    expect(backend.pasted).toEqual(['cargo test --all']);
  });

  // **Roadmap 28.3**, found on NVDA 2026.1.1 in 28's own pass: F6 in far-end-line mode did
  // nothing at all, because the toggle asked the `<input>` — hidden and unfocused there — and
  // then focused it, which is a no-op. Review by heading is the whole of how this product is
  // read back, so it has to reach the buffer from whichever line is in front.
  it('F6 reaches the buffer from the far end line, and comes back to it', async () => {
    const { buffer, farEndField, editField, controller } = await makeApp();
    await controller.toggleLineOwner();
    expect(farEndField.focused).toBe(true);
    buffer.focused = false;
    editField.focused = false;

    controller.toggleFocusArea();
    expect(buffer.focused).toBe(true);

    // And back to the far end's line rather than to the hidden local one.
    farEndField.focused = false;
    controller.toggleFocusArea();
    expect(farEndField.focused).toBe(true);
    expect(editField.focused).toBe(false);
  });

  it('Escape from the buffer returns to the far end line, not the hidden local one', async () => {
    const { buffer, farEndField, editField, controller } = await makeApp();
    await controller.toggleLineOwner();
    farEndField.focused = false;
    editField.focused = false;
    buffer.focused = true;

    controller.escapeToCommandLine();

    expect(farEndField.focused).toBe(true);
    expect(editField.focused).toBe(false);
  });
});

// **The three answers to Ctrl+D** (spec 28, decision 9; roadmap 23.5). The frontend words
// the answer to the key it sent and still does not decide what the key means: which of the
// three applies is the backend's `KeyAck`.
describe('what Ctrl+D is answered with (23.5)', () => {
  const ctrlD: KeyPress = {
    key: { Char: 'd' },
    ctrl: true,
    shift: false,
    alt: false,
  };

  // The far end took it and the session is ending. The connection sentence already says so,
  // and a second announcement would talk over the answer the user actually wanted.
  it('says nothing when the far end took it', async () => {
    const { backend, announcer, controller } = await makeApp();
    backend.keyAck = 'Applied';

    await controller.reportKey(ctrlD);

    expect(announcer.announcements).toEqual([]);
  });

  // "Bound, and this shell has no measured answer" — which is a working session, so the
  // sentence has to say what to do instead rather than only that the key did nothing.
  it('names the way out when the shell has no key for end of input', async () => {
    const { backend, announcer, controller } = await makeApp();
    backend.keyAck = 'Unsupported';

    await controller.reportKey(ctrlD);

    expect(announcer.announcements).toEqual([noEndOfInputKeyMessage]);
    expect(noEndOfInputKeyMessage).toContain('exit');
  });

  it('says the session has already ended when nothing is listening', async () => {
    const { backend, announcer, controller } = await makeApp();
    backend.keyAck = 'NothingToActOn';

    await controller.reportKey(ctrlD);

    expect(announcer.announcements).toEqual([sessionAlreadyEndedMessage]);
  });

  // And the three are three, which is the whole reason `Unsupported` was added: before it,
  // "no measured answer" and "nothing is listening" were the same reply.
  it('does not give Ctrl+C the sentences that belong to Ctrl+D', async () => {
    const ctrlC: KeyPress = {
      key: { Char: 'c' },
      ctrl: true,
      shift: false,
      alt: false,
    };
    const { backend, announcer, controller } = await makeApp();
    backend.keyAck = 'NothingToActOn';

    await controller.reportKey(ctrlC);

    expect(announcer.announcements).toEqual([nothingToStopMessage]);
  });
});

describe('focus flow', () => {
  it('F6 toggles from edit field to buffer and back', async () => {
    const { editField, buffer, controller } = await makeApp();

    editField.focused = true;
    controller.toggleFocusArea();
    expect(buffer.focused).toBe(true);

    editField.focused = false;
    controller.toggleFocusArea();
    expect(editField.focused).toBe(true);
  });

  it('Escape returns to the edit field only when the buffer has focus', async () => {
    const { editField, buffer, controller } = await makeApp();

    editField.focused = false;
    buffer.focused = false;
    controller.escapeToCommandLine();
    expect(editField.focused).toBe(false);

    buffer.focused = true;
    controller.escapeToCommandLine();
    expect(editField.focused).toBe(true);
  });
});

describe('Announce (B1.5): speech is its own event', () => {
  it('ReadAloud speaks the text without appending it — Output already did', async () => {
    const { backend, buffer, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'hello\n',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'hello\n' },
    });

    expect(buffer.appended).toEqual([{ commandId: 1, line: 1, revision: 'Appended', text: 'hello\n' }]);
    expect(announcer.announcements).toEqual(['hello\n']);
  });

  it('TooBig announces the count the backend sent, never a recount of the text', async () => {
    const { backend, buffer, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    // Past the threshold the backend does not hold the text, so the count cannot be
    // re-derived here even in principle — it is carried.
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'y\ny\n',
    });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 500 },
    });

    expect(announcer.announcements).toEqual([tooBigMessage(500)]);
    expect(buffer.appended).toEqual([{ commandId: 1, line: 1, revision: 'Appended', text: 'y\ny\n' }]);
  });

  it('a too-big Announce arms the completion beep', async () => {
    const { backend, beep, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 500 },
    });
    expect(beep.beeps).toBe(0);

    backend.emit({ type: 'CommandFinished', command_id: 1 });
    expect(beep.beeps).toBe(1);
  });

  it('StillRunning speaks the patience string', async () => {
    const { backend, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'StillRunning' },
    });

    expect(announcer.announcements).toEqual([patienceMessage]);
  });

  it('OutputContinues says the output is still arriving, not that it stopped', async () => {
    const { backend, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'OutputContinues' },
    });

    expect(announcer.announcements).toEqual([outputContinuesMessage]);
    expect(outputContinuesMessage).toContain('buffer');
  });

  it('Failed speaks the exit code', async () => {
    const { backend, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'Failed', exit_code: 2 },
    });

    expect(announcer.announcements).toEqual([failureMessage(2)]);
  });

  it('quiet Output is buffered and silent, so the babble guard withholds nothing', async () => {
    const { backend, buffer, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'OutputContinues' },
    });
    for (const text of ['still\n', 'coming\n']) {
      backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text,
    });
    }

    expect(buffer.appended).toEqual([
      { commandId: 1, line: 1, revision: 'Appended', text: 'still\n' },
      { commandId: 1, line: 1, revision: 'Appended', text: 'coming\n' },
    ]);
    expect(announcer.announcements).toEqual([outputContinuesMessage]);
  });
});

// The keystroke the frontend does not consume, and the two answers only it can voice.
// What a key *means* is the backend's table, so these choose the ack rather than the
// meaning — which is the whole point of reporting a key instead of an intent.
describe('reportKey (A3.2)', () => {
  const ctrlC: KeyPress = {
    key: { Char: 'c' },
    ctrl: true,
    shift: false,
    alt: false,
  };

  it('reports the keystroke as pressed', async () => {
    const { backend, controller } = await makeApp();

    await controller.reportKey(ctrlC);

    expect(backend.keysSent).toEqual([ctrlC]);
  });

  it('says nothing when the intent was applied: the session speaks for itself', async () => {
    const { backend, announcer, controller } = await makeApp();
    backend.keyAck = 'Applied';

    await controller.reportKey(ctrlC);

    expect(announcer.announcements).toEqual([]);
  });

  it('says there is nothing to stop when nothing was running', async () => {
    const { backend, announcer, controller } = await makeApp();
    backend.keyAck = 'NothingToActOn';

    await controller.reportKey(ctrlC);

    expect(announcer.announcements).toEqual([nothingToStopMessage]);
  });

  // Unreachable while Ctrl+C is both the only key reported and the only key bound. It is
  // tested anyway: the failure it guards against is a second reported key going silent.
  it('says an unbound key did nothing rather than going silent', async () => {
    const { backend, announcer, controller } = await makeApp();
    backend.keyAck = 'Unbound';

    await controller.reportKey(ctrlC);

    expect(announcer.announcements).toEqual([unboundKeyMessage]);
  });
});

// DESIGN's layer 2, the half the frontend enforces: the session hears a keystroke only
// while the edit field has focus and holds no selection. Focus is enforced by the
// keyboard adapter listening on the field itself, so what is left here is the selection.
describe('editFieldHasSelection (A3.2)', () => {
  it('is true when the edit field holds a selection', async () => {
    const { editField, controller } = await makeApp();
    editField.selected = true;

    expect(controller.editFieldHasSelection()).toBe(true);
  });

  it('is false with only a caret', async () => {
    const { editField, controller } = await makeApp();
    editField.selected = false;

    expect(controller.editFieldHasSelection()).toBe(false);
  });
});

// **The window B7 created**, and the one every launch now opens with unless something
// named a shell. An empty window is a state with obligations: it says it is empty, it
// answers a line typed into it, and it has nothing navigable in the buffer.
describe('a window connected to nothing (spec B7, decision 3)', () => {
  /** Startup with no session: nothing is attached, and the window says so three ways. */
  async function emptyWindow() {
    const connect = new FakeConnect();
    connect.atStartup = null;
    const backend = new FakeBackend();
    const editField = new FakeEditField();
    const buffer = new FakeBuffer();
    const announcer = new FakeAnnouncer();
    const beep = new FakeBeep();
    const window = new FakeWindow();
    const controller = new AppController(
      backend,
      connect,
      editField,
      buffer,
      new FakeFarEndField(),
      announcer,
      beep,
      window,
    );
    await controller.start();
    return { backend, connect, editField, buffer, announcer, window, controller };
  }

  it('announces that it is not connected and what to do about it', async () => {
    const { announcer, window } = await emptyWindow();

    expect(announcer.announcements).toEqual([notConnectedMessage]);
    // It names the control the listener is already on rather than a keystroke to hunt for:
    // A10 put a Connect button under focus, so the route to describe is that button.
    expect(announcer.announcements[0]).toContain('Connect');
    expect(window.statuses).toEqual(['not connected']);
    expect(window.titles).toEqual([null]);
  });

  /** **The window has no terminal in it** (spec A10): no results buffer to arrow onto and
   * hear nothing from, and no edit field that could submit nothing. */
  it('shows no terminal window at all', async () => {
    const { window } = await emptyWindow();

    expect(window.terminals).toEqual([false]);
  });

  it('attaches to nothing, because there is nothing to attach to', async () => {
    const { backend } = await emptyWindow();

    expect(backend.attachedTo).toEqual([]);
  });

  /** **The rule with teeth.** A line typed into an empty window is answered, nothing is
   * sent anywhere, no block is opened for it, and the text survives — because it is what
   * the user will press Enter on again once they have connected. */
  it('answers a submitted line, sends nothing, and keeps what was typed', async () => {
    const { backend, editField, announcer, buffer, controller } = await emptyWindow();
    editField.text = 'dir';
    announcer.announcements = [];

    await controller.submit();

    expect(backend.submitted).toEqual([]);
    expect(editField.text).toBe('dir');
    expect(editField.clearedCount).toBe(0);
    expect(buffer.opened).toEqual([]);
    expect(announcer.announcements).toEqual([notConnectedMessage]);
  });

  /** Nothing navigable in the buffer: no heading, and no empty block for a listener to
   * land on. A heading has to correspond to something that ran. */
  it('has nothing in the buffer at all', async () => {
    const { buffer } = await emptyWindow();

    expect(buffer.opened).toEqual([]);
    expect(buffer.appended).toEqual([]);
    expect(buffer.prompts).toEqual([]);
  });

  /** Ctrl+C into an empty window is answered rather than silent, with the words that are
   * already true of it: there is nothing running to stop. */
  it('answers a keystroke without a round trip', async () => {
    const { backend, announcer, controller } = await emptyWindow();
    announcer.announcements = [];

    await controller.reportKey({
      key: { Char: 'c' },
      ctrl: true,
      shift: false,
      alt: false,
    });

    expect(backend.keysSent).toEqual([]);
    expect(announcer.announcements).toEqual([nothingToStopMessage]);
  });
});

// Connecting, from the frontend's side: what it does with the two answers `use` can give.
describe('connecting to a profile (spec B7)', () => {
  const ubuntu: ProfileId = { profile: 'Distribution', name: 'Ubuntu' };

  it('clears the buffer before attaching, so no shell writes under another one', async () => {
    const { backend, buffer, controller } = await makeApp();
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'from the old shell',
    });
    expect(buffer.appended).toHaveLength(1);

    await controller.connectTo(ubuntu);

    expect(buffer.cleared).toBeGreaterThan(0);
    expect(buffer.appended).toEqual([]);
  });

  it('attaches to the new session and names the window after it', async () => {
    const { backend, window, announcer, controller } = await makeApp();

    await controller.connectTo(ubuntu);
    controller.announceConnection();

    expect(backend.attachedTo).toEqual([1, 2]);
    expect(window.titles).toEqual(['WSL: Ubuntu']);
    expect(announcer.announcements).toEqual([connectedMessage('WSL: Ubuntu')]);
  });

  /**
   * **Connecting does not say where you are; `announceConnection` does** (spec 13.3).
   *
   * Said while the connect dialogs were still up, the sentence went into a live region that
   * was about to be taken away — or into the document's in the same millisecond it came back
   * — and either way it was never spoken. So the words stay here and the moment belongs to
   * whoever closed the dialog.
   */
  it('says nothing at the moment it connects, and says it when asked', async () => {
    const { announcer, controller } = await makeApp();

    await controller.connectTo(ubuntu);
    expect(announcer.announcements).toEqual([]);

    expect(controller.announceConnection()).toBe(true);
    expect(announcer.announcements).toEqual([connectedMessage('WSL: Ubuntu')]);
  });

  /** And there is nothing to say about a window connected to nothing. */
  it('says nothing when there is no connection to name', async () => {
    const connect = new FakeConnect();
    connect.atStartup = null;
    const { announcer, controller } = await makeApp(connect);

    expect(controller.announceConnection()).toBe(false);
    expect(announcer.announcements).toEqual([]);
  });

  /**
   * **One sentence rather than two** (spec B9, decision 7).
   *
   * An SSH far end is asked what it is *before* the session channel is opened, so the
   * answer is in hand before there is anything to announce. What a listener hears is the
   * connection, the far end, and the state of it, in one utterance — where a bare "shell
   * integration unavailable" arriving two seconds later names nothing and speaks over
   * whatever was being said.
   */
  it('says what the far end is in the same sentence as the connection', async () => {
    const { connect, announcer, controller } = await makeApp();
    connect.note = 'zsh, which Acter cannot set up yet.';

    await controller.connectTo(ubuntu);
    controller.announceConnection();

    expect(announcer.announcements).toEqual([
      'connected to WSL: Ubuntu, zsh, which Acter cannot set up yet.',
    ]);
  });

  /**
   * And having said it, the session does not say it again.
   *
   * `IntegrationUnavailable` fires when the startup grace period expires with no markers,
   * which for an unintegrated far end is certain rather than diagnostic. Hearing the same
   * fact twice, the second time without the far end's name, teaches a listener that Acter
   * repeats itself.
   */
  it('does not repeat the integration warning the connection already gave', async () => {
    const { backend, connect, announcer, controller } = await makeApp();
    connect.note = 'zsh, which Acter cannot set up yet.';
    connect.limitExplained = true;
    await controller.connectTo(ubuntu);
    announcer.announcements.length = 0;

    backend.emit({ type: 'IntegrationUnavailable' });

    expect(announcer.announcements).toEqual([]);
  });

  /** A connection that said nothing about integration leaves the session free to say it. */
  it('still warns when the connection said nothing about integration', async () => {
    const { backend, announcer, controller } = await makeApp();
    await controller.connectTo(ubuntu);
    announcer.announcements.length = 0;

    backend.emit({ type: 'IntegrationUnavailable' });

    expect(announcer.announcements).toEqual([integrationUnavailableMessage]);
  });

  /**
   * **Progress is said while it happens** (spec B9, decision 6), because a listener with no
   * feedback cannot tell a slow network from a dead one — and an SSH connection can take
   * seconds before anything is certain.
   */
  it('says what a connection is doing while it does it', async () => {
    const { connect, announcer, controller } = await makeApp();
    connect.progress = ['Connecting to acter-ssh.', 'Signing in.'];

    await controller.connectTo(ubuntu);

    expect(announcer.announcements.slice(0, 2)).toEqual([
      'Connecting to acter-ssh.',
      'Signing in.',
    ]);
  });

  /** The id is what every later invoke names, so a line submitted after connecting goes to
   * the session that was just started rather than the one it replaced. */
  it('submits into the session it just connected to', async () => {
    const { backend, editField, controller } = await makeApp();
    await controller.connectTo(ubuntu);
    editField.text = 'ls';

    await controller.submit();

    expect(backend.submittedTo).toEqual([2]);
  });

  /** **A failure costs the user nothing.** The sentence is the backend's own, the window
   * still says what it was on, and the session that was running still takes a line. */
  it('says why a connection failed and leaves the running session alone', async () => {
    const connect = new FakeConnect();
    const { backend, window, announcer, editField, controller } = await makeApp(connect);
    connect.refuses =
      'PowerShell 7 is not installed. Install it by running winget install Microsoft.PowerShell from any terminal.';

    await controller.connectTo({ profile: 'Shell', kind: 'PowerShellSeven' });

    expect(announcer.announcements).toEqual([connect.refuses]);
    expect(window.titles).toEqual([]);
    expect(backend.attachedTo).toEqual([1]);

    editField.text = 'dir';
    await controller.submit();
    expect(backend.submittedTo).toEqual([1]);
  });

  /** The session went away between the keypress and the invoke — replaced, or ended. The
   * answer is the unconnected window's, for the same reason: nothing ran anywhere, so the
   * text stays where the user can send it again. */
  it('answers a refused submission and keeps the text', async () => {
    const { backend, editField, announcer, controller } = await makeApp();
    backend.refuseSubmissions = true;
    editField.text = 'dir';

    await controller.submit();

    expect(announcer.announcements).toEqual([notConnectedMessage]);
    expect(editField.text).toBe('dir');
    expect(editField.clearedCount).toBe(0);
  });

  it('hands the connect list on for whoever is rendering it', async () => {
    const { connect, controller } = await makeApp();

    expect(await controller.connectable()).toEqual(connect.rows);
  });
});

// **The window has two faces** (spec A10), and which one it shows follows the session
// rather than anything the user did. The terminal window — a results buffer and an edit
// field — belongs to a session; with none there is a Connect button and nothing to type
// into.
describe('the two faces of the window (spec A10)', () => {
  it('brings the terminal window up when a profile is used', async () => {
    const connect = new FakeConnect();
    connect.atStartup = null;
    const { window, controller } = await makeApp(connect);

    await controller.connectTo({ profile: 'Shell', kind: 'Cmd' });

    expect(window.terminals).toEqual([true]);
  });

  it('shows it before attaching, so nothing arrives at a window of the wrong shape', async () => {
    const connect = new FakeConnect();
    connect.atStartup = null;
    const { backend, window, controller } = await makeApp(connect);

    await controller.connectTo({ profile: 'Shell', kind: 'Cmd' });

    expect(window.terminals).toEqual([true]);
    expect(backend.attachedTo).toEqual([2]);
  });

  /** **The disconnect rule.** The buffer is the record of a session that ended, and a user
   * who typed `exit` by accident must not lose it; the edit field has nothing left to
   * submit to, so it goes. */
  it('takes the edit field away when the far end goes, and keeps the buffer', async () => {
    const { backend, buffer, window, controller } = await makeApp();
    backend.emit({
      type: 'Output',
      command_id: 1,
      line: 1,
      revision: 'Appended',
      text: 'some history',
    });
    const before = buffer.cleared;

    backend.emit({ type: 'ConnectionChanged', state: 'Disconnected' });

    expect(window.terminals).toEqual([false]);
    expect(buffer.cleared).toBe(before);
    expect(buffer.appended).toHaveLength(1);
    expect(window.statuses).toContain('not connected');
    expect(window.titles).toContain(null);
    expect(controller.editFieldHasSelection()).toBe(false);
  });

  it('says it is not connected when the far end goes', async () => {
    const { backend, announcer } = await makeApp();
    announcer.announcements = [];

    backend.emit({ type: 'ConnectionChanged', state: 'Disconnected' });

    expect(announcer.announcements).toEqual([notConnectedMessage]);
  });

  /** And a line typed into what is left is refused rather than run: the session is gone, so
   * the controller no longer names one. */
  it('refuses a line after the far end has gone', async () => {
    const { backend, editField, announcer, controller } = await makeApp();
    backend.emit({ type: 'ConnectionChanged', state: 'Disconnected' });
    announcer.announcements = [];
    editField.text = 'dir';

    await controller.submit();

    expect(backend.submitted).toEqual([]);
    expect(editField.text).toBe('dir');
    expect(announcer.announcements).toEqual([notConnectedMessage]);
  });
});
