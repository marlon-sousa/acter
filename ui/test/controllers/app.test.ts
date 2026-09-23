// Role: test — controller behavior against fake backend, views, and beep.

import { describe, expect, it } from 'vitest';

import type {
  CommandId,
  Connectable,
  Connected,
  KeyAck,
  KeyPress,
  LaunchRequest,
  LineId,
  LineOwner,
  LineRevision,
  ProfileId,
  SavedConnections,
  SavedRow,
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
  keysGoToActerMessage,
  keysGoToTheProgramMessage,
} from '../../src/controllers/app';

class FakeBackend implements BackendApi {
  submitted: string[] = [];
  private nextId = 1;
  private onEvent: ((event: SessionEvent) => void) | undefined;

  attachedTo: SessionId[] = [];
  attachSession(
    session: SessionId,
    onEvent: (event: SessionEvent) => void,
  ): Promise<void> {
    this.attachedTo.push(session);
    this.onEvent = onEvent;
    return Promise.resolve();
  }
  deferAcks = false;
  private held: Array<() => void> = [];
  refuseSubmissions = false;
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
  keyAck: KeyAck = 'Applied';
  keysSent: KeyPress[] = [];
  sendKey(_session: SessionId, key: KeyPress): Promise<KeyAck> {
    this.keysSent.push(key);
    return Promise.resolve(this.keyAck);
  }
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
  returns = 0;
  announce(text: string): void {
    this.announcements.push(text);
  }
  documentReturned(): void {
    this.returns += 1;
  }
  drained(): Promise<void> {
    return Promise.resolve();
  }
}

class FakeBeep implements BeepView {
  beeps = 0;
  beep(): void {
    this.beeps += 1;
  }
}

class FakeWindow implements WindowView {
  titles: Array<string | null> = [];
  statuses: string[] = [];
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
  localLines: boolean[] = [];
  showLocalLine(showing: boolean): void {
    this.localLines.push(showing);
  }
}

class FakeFarEndField implements FarEndFieldView {
  rendered: Array<{ text: string | null; caret: number; completed: boolean }> = [];
  showing = false;
  focused = false;
  render(text: string | null, caret: number, completed = false): void {
    this.rendered.push({ text, caret, completed });
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
    saved_as: null,
    line_owner: 'FarEnd',
  };
  note: string | null = null;
  limitExplained = false;
  setUps: SetUp[] = [];
  progress: string[] = [];
  refuses: string | null = null;
  used: ProfileId[] = [];
  origins: (string | null)[] = [];
  lineOwner: LineOwner = 'FarEnd';
  savedRows: SavedRow[] = [];
  unreadable: string | null = null;
  offering = true;
  stopped = 0;
  savedUnder: string[] = [];
  refusesSaving: string | null = null;
  atLaunch: LaunchRequest | null = null;
  private nextSession = 1;

  connectable(): Promise<Connectable[]> {
    return Promise.resolve(this.rows);
  }
  use(
    id: ProfileId,
    setUp: SetUp,
    origin: string | null,
    listener?: ConnectListener,
  ): Promise<Connected> {
    this.used.push(id);
    this.setUps.push(setUp);
    this.origins.push(origin);
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
      saved_as: origin,
      line_owner: this.lineOwner,
    });
  }
  connected(): Promise<Connected | null> {
    return Promise.resolve(this.atStartup);
  }
  saved(): Promise<SavedConnections> {
    return Promise.resolve({ rows: this.savedRows, unreadable: this.unreadable });
  }
  saveConnection(name: string): Promise<string> {
    if (this.refusesSaving !== null) {
      return Promise.reject(this.refusesSaving);
    }
    this.savedUnder.push(name);
    return Promise.resolve(`Saved as ${name}.`);
  }
  renameConnection(from: string, to: string): Promise<string> {
    return Promise.resolve(`${from} is now called ${to}.`);
  }
  forgetConnection(name: string): Promise<string> {
    return Promise.resolve(`${name} is no longer saved.`);
  }
  offerToSave(): Promise<boolean> {
    return Promise.resolve(this.offering);
  }
  stopOfferingToSave(): Promise<void> {
    this.stopped += 1;
    return Promise.resolve();
  }
  requestedAtLaunch(): Promise<LaunchRequest | null> {
    return Promise.resolve(this.atLaunch);
  }
}

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
  it('says it is connecting', async () => {
    const { backend, window, controller } = await makeApp();

    backend.emit({ type: 'ConnectionChanged', state: 'Connecting' });

    expect(window.statuses).toContain('connecting');
  });

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

  it('restates the same sentence when the far end reports it is connected', async () => {
    const ubuntu: ProfileId = { profile: 'Distribution', name: 'Ubuntu' };
    const { backend, window, controller } = await makeApp();
    await controller.connectTo(ubuntu);
    window.statuses.length = 0;

    backend.emit({ type: 'ConnectionChanged', state: 'Connected' });

    expect(window.statuses).toContain('connected to WSL: Ubuntu');
  });

  it('says it is not connected when the far end goes away, and drops the name', async () => {
    const { backend, window, controller } = await makeApp();

    backend.emit({ type: 'ConnectionChanged', state: 'Disconnected' });

    expect(window.statuses).toContain('not connected');
    expect(window.titles).toContain(null);
  });

  it('treats reconnecting as connecting rather than falling silent', async () => {
    const { backend, window, controller } = await makeApp();

    backend.emit({ type: 'ConnectionChanged', state: 'Reconnecting' });

    expect(window.statuses).toContain('connecting');
  });
});

describe('the prompt a marked shell drew (spec B5.6)', () => {
  it('speaks the prompt and keeps it in the buffer', async () => {
    const { backend, buffer, announcer, controller } = await makeApp();

    backend.emit({ type: 'PromptDrawn', text: 'C:\projects\acter (main)>' });

    expect(buffer.prompts).toEqual(['C:\projects\acter (main)>']);
    expect(announcer.announcements).toContain('C:\projects\acter (main)>');
  });

  it('speaks an unchanged prompt again rather than falling silent', async () => {
    const { backend, announcer, controller } = await makeApp();

    backend.emit({ type: 'PromptDrawn', text: 'acter>' });
    backend.emit({ type: 'PromptDrawn', text: 'acter>' });

    expect(announcer.announcements.filter((said) => said === 'acter>')).toHaveLength(2);
  });

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
      drained: () => Promise.resolve(),
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

    expect(announcer.announcements).toEqual(['error: boom', failureMessage(2)]);
    expect(beep.beeps).toBe(0);
  });

  it('a failing command that finishes says nothing until the Failed announcement', async () => {
    const { backend, announcer, controller } = await makeApp();

    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    expect(announcer.announcements).toEqual([]);
  });

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
    expect(announcer.announcements[0]).toBe(
      'You will hear what commands print here, but not whether they worked. Press F1 for help.',
    );
    expect(buffer.opened).toEqual([]);
  });

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
    const { backend, buffer, editField, controller } = await makeApp();
    backend.emit({ type: 'CommandStarted', command_id: 1, command_line: null });

    editField.text = 'small';
    await controller.submit();

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

describe('handing the line to the far end (28)', () => {
  it('hands the keys to the program as soon as there is a session', async () => {
    const { backend, farEndField } = await makeApp();

    expect(backend.owners).toEqual(['FarEnd']);
    expect(farEndField.showing).toBe(true);
  });

  it('takes it back, and says what is gained and lost', async () => {
    const { backend, editField, farEndField, window, announcer, controller } =
      await makeApp();
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

  it('hands them over again, and says that too', async () => {
    const { backend, farEndField, window, announcer, controller } = await makeApp();
    await controller.toggleLineOwner();
    announcer.announcements = [];
    window.localLines = [];
    farEndField.focused = false;

    await controller.toggleLineOwner();

    expect(backend.owners).toEqual(['FarEnd', 'Local', 'FarEnd']);
    expect(farEndField.showing).toBe(true);
    expect(farEndField.focused).toBe(true);
    expect(window.localLines).toEqual([false]);
    expect(announcer.announcements).toEqual([farEndLineOnMessage]);
  });

  it('names the state without promising anything Acter does not have', () => {
    for (const said of [farEndLineOnMessage, farEndLineOffMessage]) {
      expect(said.toLowerCase()).not.toContain('mode');
      expect(said.toLowerCase()).not.toContain('history');
      expect(said.toLowerCase()).not.toContain('completion');
      expect(said.length).toBeLessThan(30);
    }
  });

  it('says which way it went', () => {
    expect(farEndLineOnMessage).not.toBe(farEndLineOffMessage);
  });

  it('names the two parties and the key, and teaches no jargon', () => {
    for (const said of [keysGoToTheProgramMessage, keysGoToActerMessage]) {
      expect(said).toContain('Ctrl+Shift+K');
      expect(said.toLowerCase()).toContain('process keys');
      expect(said.toLowerCase()).not.toContain('mode');
      expect(said.toLowerCase()).not.toContain('far end');
      expect(said.toLowerCase()).not.toContain('local');
    }
    expect(keysGoToTheProgramMessage).not.toBe(keysGoToActerMessage);
  });

  it('refuses in a window with no session, in the words that window already uses', async () => {
    const connect = new FakeConnect();
    connect.atStartup = null;
    const { announcer, controller, backend } = await makeApp(connect);

    await controller.toggleLineOwner();

    expect(backend.owners).toEqual([]);
    expect(announcer.announcements).toEqual([notConnectedMessage]);
  });

  it('comes back to Acter when the far end goes away, silently', async () => {
    const { farEndField, announcer, backend, controller } = await makeApp();
    await controller.toggleLineOwner();
    announcer.announcements = [];

    backend.emit({ type: 'ConnectionChanged', state: 'Disconnected' });

    expect(farEndField.showing).toBe(false);
    expect(announcer.announcements).toEqual([notConnectedMessage]);
  });

  it('hands the row and the caret to the field and announces neither', async () => {
    const { backend, farEndField, announcer, controller } = await makeApp();
    await controller.toggleLineOwner();
    announcer.announcements = [];

    backend.emit({ type: 'FarEndLine', text: 'cargo test --all', caret: 16 });
    backend.emit({ type: 'FarEndLine', text: null, caret: 3 });

    expect(farEndField.rendered).toEqual([
      { text: 'cargo test --all', caret: 16, completed: false },
      { text: null, caret: 3, completed: false },
    ]);
    expect(announcer.announcements).toEqual([]);
  });

  it('marks the answer to a completion, and only that one', async () => {
    const { backend, farEndField, controller } = await makeApp();
    await controller.toggleLineOwner();

    await controller.reportKey({ key: 'Tab', ctrl: false, shift: false, alt: false });
    backend.emit({ type: 'FarEndLine', text: 'echo ', caret: 5 });
    backend.emit({ type: 'FarEndLine', text: 'echo one', caret: 8 });

    expect(farEndField.rendered).toEqual([
      { text: 'echo ', caret: 5, completed: true },
      { text: 'echo one', caret: 8, completed: false },
    ]);
  });

  it('does not mark the answer to a key the reader speaks for', async () => {
    const { backend, farEndField, controller } = await makeApp();
    await controller.toggleLineOwner();

    await controller.reportKey({ key: 'Tab', ctrl: false, shift: false, alt: false });
    await controller.reportKey({ key: 'Up', ctrl: false, shift: false, alt: false });
    backend.emit({ type: 'FarEndLine', text: 'echo one', caret: 8 });

    expect(farEndField.rendered).toEqual([
      { text: 'echo one', caret: 8, completed: false },
    ]);
  });

  it('pastes into the far end only while the far end owns the line', async () => {
    const { backend, controller } = await makeApp();

    await controller.pasteToFarEnd('cargo test --all');
    expect(backend.pasted).toEqual(['cargo test --all']);

    await controller.toggleLineOwner();
    await controller.pasteToFarEnd('ignored');

    expect(backend.pasted).toEqual(['cargo test --all']);
  });

  it('F6 reaches the buffer from the far end line, and comes back to it', async () => {
    const { buffer, farEndField, editField, controller } = await makeApp();
    expect(farEndField.focused).toBe(true);
    buffer.focused = false;
    editField.focused = false;

    controller.toggleFocusArea();
    expect(buffer.focused).toBe(true);

    farEndField.focused = false;
    controller.toggleFocusArea();
    expect(farEndField.focused).toBe(true);
    expect(editField.focused).toBe(false);
  });

  it('Escape from the buffer returns to the far end line, not the hidden local one', async () => {
    const { buffer, farEndField, editField, controller } = await makeApp();
    farEndField.focused = false;
    editField.focused = false;
    buffer.focused = true;

    controller.escapeToCommandLine();

    expect(farEndField.focused).toBe(true);
    expect(editField.focused).toBe(false);
  });
});

describe('what Ctrl+D is answered with (23.5)', () => {
  const ctrlD: KeyPress = {
    key: { Char: 'd' },
    ctrl: true,
    shift: false,
    alt: false,
  };

  it('says nothing when the far end took it', async () => {
    const { backend, announcer, controller } = await makeApp();
    backend.keyAck = 'Applied';

    await controller.reportKey(ctrlD);

    expect(announcer.announcements).toEqual([]);
  });

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
    await controller.toggleLineOwner();

    editField.focused = true;
    controller.toggleFocusArea();
    expect(buffer.focused).toBe(true);

    editField.focused = false;
    controller.toggleFocusArea();
    expect(editField.focused).toBe(true);
  });

  it('Escape returns to the edit field only when the buffer has focus', async () => {
    const { editField, buffer, controller } = await makeApp();
    await controller.toggleLineOwner();

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

  it('says an unbound key did nothing rather than going silent', async () => {
    const { backend, announcer, controller } = await makeApp();
    backend.keyAck = 'Unbound';

    await controller.reportKey(ctrlC);

    expect(announcer.announcements).toEqual([unboundKeyMessage]);
  });
});

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

describe('a window connected to nothing (spec B7, decision 3)', () => {
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
    expect(announcer.announcements[0]).toContain('Connect');
    expect(window.statuses).toEqual(['not connected']);
    expect(window.titles).toEqual([null]);
  });

  it('shows no terminal window at all', async () => {
    const { window } = await emptyWindow();

    expect(window.terminals).toEqual([false]);
  });

  it('attaches to nothing, because there is nothing to attach to', async () => {
    const { backend } = await emptyWindow();

    expect(backend.attachedTo).toEqual([]);
  });

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

  it('has nothing in the buffer at all', async () => {
    const { buffer } = await emptyWindow();

    expect(buffer.opened).toEqual([]);
    expect(buffer.appended).toEqual([]);
    expect(buffer.prompts).toEqual([]);
  });

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
    expect(announcer.announcements).toEqual([
      connectedMessage('WSL: Ubuntu'),
      keysGoToTheProgramMessage,
    ]);
  });

  it('says nothing at the moment it connects, and says it when asked', async () => {
    const { announcer, controller } = await makeApp();

    await controller.connectTo(ubuntu);
    expect(announcer.announcements).toEqual([]);

    expect(controller.announceConnection()).toBe(true);
    expect(announcer.announcements).toEqual([
      connectedMessage('WSL: Ubuntu'),
      keysGoToTheProgramMessage,
    ]);
  });

  it('says nothing when there is no connection to name', async () => {
    const connect = new FakeConnect();
    connect.atStartup = null;
    const { announcer, controller } = await makeApp(connect);

    expect(controller.announceConnection()).toBe(false);
    expect(announcer.announcements).toEqual([]);
  });

  it('says what the far end is in the same sentence as the connection', async () => {
    const { connect, announcer, controller } = await makeApp();
    connect.note = 'zsh, which Acter cannot set up yet.';

    await controller.connectTo(ubuntu);
    controller.announceConnection();

    expect(announcer.announcements).toEqual([
      'connected to WSL: Ubuntu, zsh, which Acter cannot set up yet.',
      keysGoToTheProgramMessage,
    ]);
  });

  it('does not repeat the integration warning the connection already gave', async () => {
    const { backend, connect, announcer, controller } = await makeApp();
    connect.note = 'zsh, which Acter cannot set up yet.';
    connect.limitExplained = true;
    await controller.connectTo(ubuntu);
    announcer.announcements.length = 0;

    backend.emit({ type: 'IntegrationUnavailable' });

    expect(announcer.announcements).toEqual([]);
  });

  it('still warns when the connection said nothing about integration', async () => {
    const { backend, announcer, controller } = await makeApp();
    await controller.connectTo(ubuntu);
    announcer.announcements.length = 0;

    backend.emit({ type: 'IntegrationUnavailable' });

    expect(announcer.announcements).toEqual([integrationUnavailableMessage]);
  });

  it('says what a connection is doing while it does it', async () => {
    const { connect, announcer, controller } = await makeApp();
    connect.progress = ['Connecting to acter-ssh.', 'Signing in.'];

    await controller.connectTo(ubuntu);

    expect(announcer.announcements.slice(0, 2)).toEqual([
      'Connecting to acter-ssh.',
      'Signing in.',
    ]);
  });

  it('submits into the session it just connected to', async () => {
    const { backend, editField, controller } = await makeApp();
    await controller.connectTo(ubuntu);
    editField.text = 'ls';

    await controller.submit();

    expect(backend.submittedTo).toEqual([2]);
  });

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

describe('the connections somebody saved', () => {
  it('opens on the line the saved connection asked for', async () => {
    const connect = new FakeConnect();
    connect.lineOwner = 'Local';
    const { backend, controller, farEndField } = await makeApp(connect);

    await controller.connectTo({ profile: 'Shell', kind: 'Cmd' }, 'Yes', 'quiet');

    expect(backend.owners.at(-1)).toBe('Local');
    expect(farEndField.showing).toBe(false);
    expect(connect.origins.at(-1)).toBe('quiet');
  });

  it('opens a new connection on the far end line, with no origin', async () => {
    const connect = new FakeConnect();
    const { backend, controller } = await makeApp(connect);

    await controller.connectTo({ profile: 'Shell', kind: 'Cmd' });

    expect(backend.owners.at(-1)).toBe('FarEnd');
    expect(connect.origins.at(-1)).toBe(null);
  });

  it('does not open the offer until the announcer has said everything', async () => {
    const connect = new FakeConnect();
    const { controller, announcer } = await makeApp(connect);
    await controller.connectTo({ profile: 'Shell', kind: 'Cmd' });
    const order: string[] = [];
    announcer.drained = () => {
      order.push('drained');
      return Promise.resolve();
    };

    await controller.offerToSave(async () => {
      order.push('offered');
      return { stopOffering: false };
    });

    expect(order).toEqual(['drained', 'offered']);
  });

  it('offers to save a new connection', async () => {
    const connect = new FakeConnect();
    const { controller } = await makeApp(connect);
    await controller.connectTo({ profile: 'Shell', kind: 'Cmd' });
    const offered: string[] = [];

    await controller.offerToSave(async (connection) => {
      offered.push(connection.label);
      return { stopOffering: false };
    });

    expect(offered).toEqual(['Command Prompt']);
  });

  it('does not offer to save a connection that already has a name', async () => {
    const connect = new FakeConnect();
    const { controller } = await makeApp(connect);
    await controller.connectTo({ profile: 'Shell', kind: 'Cmd' }, 'Yes', 'work laptop');
    let asked = 0;

    await controller.offerToSave(async () => {
      asked += 1;
      return { stopOffering: false };
    });

    expect(asked).toBe(0);
  });

  it('does not offer when the preference says not to', async () => {
    const connect = new FakeConnect();
    connect.offering = false;
    const { controller } = await makeApp(connect);
    await controller.connectTo({ profile: 'Shell', kind: 'Cmd' });
    let asked = 0;

    await controller.offerToSave(async () => {
      asked += 1;
      return { stopOffering: false };
    });

    expect(asked).toBe(0);
  });

  it('records the preference when the box was ticked', async () => {
    const connect = new FakeConnect();
    const { controller } = await makeApp(connect);
    await controller.connectTo({ profile: 'Shell', kind: 'Cmd' });

    await controller.offerToSave(async () => ({ stopOffering: true }));

    expect(connect.stopped).toBe(1);
  });

  it('saves under a name and stops offering that session', async () => {
    const connect = new FakeConnect();
    const { controller } = await makeApp(connect);
    await controller.connectTo({ profile: 'Shell', kind: 'Cmd' });

    const said = await controller.saveConnection('work laptop');

    expect(said).toBe('Saved as work laptop.');
    expect(connect.savedUnder).toEqual(['work laptop']);
    expect(controller.connectedNow?.saved_as).toBe('work laptop');

    let asked = 0;
    await controller.offerToSave(async () => {
      asked += 1;
      return { stopOffering: false };
    });
    expect(asked).toBe(0);
  });

  it('announces a refusal and answers nothing', async () => {
    const connect = new FakeConnect();
    connect.refusesSaving =
      'A connection named work laptop already exists. Choose another name, or forget that one first.';
    const { controller, announcer } = await makeApp(connect);
    await controller.connectTo({ profile: 'Shell', kind: 'Cmd' });
    announcer.announcements = [];

    const said = await controller.saveConnection('work laptop');

    expect(said).toBe(null);
    expect(announcer.announcements).toEqual([connect.refusesSaving]);
    expect(controller.connectedNow?.saved_as).toBe(null);
  });

  it('starts the saved connection a launch asked for', async () => {
    const connect = new FakeConnect();
    connect.atStartup = null;
    connect.atLaunch = { request: 'Connect', name: 'work laptop' };
    connect.savedRows = [
      {
        name: 'work laptop',
        id: { profile: 'Shell', kind: 'Cmd' },
        summary: 'Command Prompt',
        set_up: 'No',
        line_owner: 'FarEnd',
        available: true,
        instructions: null,
      },
    ];
    const { controller } = await makeApp(connect);

    const started = await controller.carryOutTheLaunchSwitch();

    expect(started).toBe(true);
    expect(connect.used).toEqual([{ profile: 'Shell', kind: 'Cmd' }]);
    expect(connect.setUps).toEqual(['No']);
    expect(connect.origins).toEqual(['work laptop']);
  });

  it('says a launch name nothing is saved under, and starts nothing', async () => {
    const connect = new FakeConnect();
    connect.atStartup = null;
    connect.atLaunch = {
      request: 'Unknown',
      name: 'wrok laptop',
      said: 'There is no saved connection named wrok laptop.',
    };
    const { controller, announcer } = await makeApp(connect);
    announcer.announcements = [];

    const started = await controller.carryOutTheLaunchSwitch();

    expect(started).toBe(false);
    expect(connect.used).toEqual([]);
    expect(announcer.announcements).toEqual([
      'There is no saved connection named wrok laptop.',
    ]);
  });

  it('does nothing at all for an ordinary launch', async () => {
    const connect = new FakeConnect();
    connect.atStartup = null;
    const { controller, announcer } = await makeApp(connect);
    announcer.announcements = [];

    expect(await controller.carryOutTheLaunchSwitch()).toBe(false);
    expect(announcer.announcements).toEqual([]);
  });
});
