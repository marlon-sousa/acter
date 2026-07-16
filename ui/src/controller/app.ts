// Role: controller — translates UI intents into backend calls and view updates.
// Framework-free: sees only the BackendApi port and the view ports.

import type { BackendApi } from '../ipc/backend';
import type { AnnouncerView, BufferView, EditFieldView } from '../views/ports';

export class AppController {
  constructor(
    private readonly backend: BackendApi,
    private readonly editField: EditFieldView,
    private readonly buffer: BufferView,
    private readonly announcer: AnnouncerView,
  ) {}

  async submit(): Promise<void> {
    const text = this.editField.value().trim();
    if (text === '') {
      return;
    }
    const response = await this.backend.echo(text);
    this.buffer.appendBlock(text, response);
    this.announcer.announce(response);
    this.editField.clear();
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
