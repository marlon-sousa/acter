// Role: adapter (DOM) — the About dialog: fill it from the build, open it modally, and
// put focus back in the edit field when it closes.
//
// The platform does the parts that matter and does them better than we would: a modal
// `<dialog>` is announced as a dialog, traps focus while it is open, and closes on
// Escape. What is left is what it cannot know — where focus belongs afterwards, which is
// the edit field, because what opened this was a menu that no longer exists (spec A7,
// decision 3).

import { keepTabInside } from './dialog_tab';
import type { AppShell } from '../ports/app_shell';

export class AboutDialog {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly shell: AppShell,
    private readonly returnTo: { focus(): void },
  ) {
    this.dialog.addEventListener('close', () => this.returnTo.focus());
    this.dialog
      .querySelector('#about-close')
      ?.addEventListener('click', () => this.dialog.close());
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
  }

  async open(): Promise<void> {
    // Opening an open dialog throws `InvalidStateError`, and the throw is silent: the
    // promise rejects into a `void` call and the user is left with whatever was on screen.
    // A menu that is asked twice — a double Enter, a click on an item already chosen — is
    // an ordinary thing, so this answers it rather than breaking.
    if (this.dialog.open) {
      return;
    }
    const facts = await this.shell.about();
    this.fill('#about-name', facts.name);
    this.fill('#about-version', `Version ${facts.version}`);
    this.fill('#about-copyright', facts.copyright);
    this.fill('#about-licence', `${facts.licence} licence`);
    this.dialog.showModal();
  }

  private fill(selector: string, text: string): void {
    const element = this.dialog.querySelector(selector);
    if (element !== null) {
      element.textContent = text;
    }
  }
}
