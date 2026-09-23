// Role: adapter (DOM) — the About dialog: fill it from the build, open it modally, and
// put focus back in the edit field when it closes.

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
    if (this.dialog.open) {
      return;
    }
    const facts = await this.shell.about();
    this.fill('#about-name', facts.name);
    this.fill('#about-version', `${facts.version_said} ${facts.version}`);
    this.fill('#about-copyright', facts.copyright);
    this.fill('#about-licence', `${facts.licence} licence`);
    this.fill(
      '#about-settings',
      `Settings folder: ${facts.settings_folder}. ${facts.settings_standing}`,
    );
    this.dialog.showModal();
  }

  private fill(selector: string, text: string): void {
    const element = this.dialog.querySelector(selector);
    if (element !== null) {
      element.textContent = text;
    }
  }
}
