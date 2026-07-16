// Role: test — controller behavior against fake backend and fake views.

import { describe, expect, it } from 'vitest';

import type { AnnouncerView } from '../../src/ports/announcer_view';
import type { BackendApi } from '../../src/ports/backend_api';
import type { BufferView } from '../../src/ports/buffer_view';
import type { EditFieldView } from '../../src/ports/edit_field_view';
import { AppController } from '../../src/controllers/app';

class FakeBackend implements BackendApi {
  calls: string[] = [];
  echo(text: string): Promise<string> {
    this.calls.push(text);
    return Promise.resolve(`response for ${text}`);
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
}

class FakeBuffer implements BufferView {
  blocks: Array<{ command: string; response: string }> = [];
  focused = false;
  appendBlock(command: string, response: string): void {
    this.blocks.push({ command, response });
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
  announce(text: string): void {
    this.announcements.push(text);
  }
}

function makeApp() {
  const backend = new FakeBackend();
  const editField = new FakeEditField();
  const buffer = new FakeBuffer();
  const announcer = new FakeAnnouncer();
  const controller = new AppController(backend, editField, buffer, announcer);
  return { backend, editField, buffer, announcer, controller };
}

describe('submit', () => {
  it('sends trimmed text, appends the block, announces, clears the field', async () => {
    const { backend, editField, buffer, announcer, controller } = makeApp();
    editField.text = '  git status  ';

    await controller.submit();

    expect(backend.calls).toEqual(['git status']);
    expect(buffer.blocks).toEqual([
      { command: 'git status', response: 'response for git status' },
    ]);
    expect(announcer.announcements).toEqual(['response for git status']);
    expect(editField.clearedCount).toBe(1);
  });

  it('ignores empty and whitespace-only input', async () => {
    const { backend, buffer, announcer, controller, editField } = makeApp();
    editField.text = '   ';

    await controller.submit();

    expect(backend.calls).toEqual([]);
    expect(buffer.blocks).toEqual([]);
    expect(announcer.announcements).toEqual([]);
    expect(editField.clearedCount).toBe(0);
  });
});

describe('focus flow', () => {
  it('F6 toggles from edit field to buffer and back', () => {
    const { editField, buffer, controller } = makeApp();

    editField.focused = true;
    controller.toggleFocusArea();
    expect(buffer.focused).toBe(true);

    editField.focused = false;
    controller.toggleFocusArea();
    expect(editField.focused).toBe(true);
  });

  it('Escape returns to the edit field only when the buffer has focus', () => {
    const { editField, buffer, controller } = makeApp();

    editField.focused = false;
    buffer.focused = false;
    controller.escapeToEditField();
    expect(editField.focused).toBe(false);

    buffer.focused = true;
    controller.escapeToEditField();
    expect(editField.focused).toBe(true);
  });
});
