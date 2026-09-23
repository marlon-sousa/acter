// Role: adapter (DOM) — the New connection dialog: a list of connection kinds, the shared
// panel holding whatever the chosen kind needs, and the three steps of connecting.

import { ConnectionPanel, worthSaying } from './connection_panel';
import { keepTabInside } from './dialog_tab';
import { OptionList } from './option_list';
import type { AnnouncerView } from '../ports/announcer_view';
import type { ConnectApi } from '../ports/connect_api';
import type { HelpView } from '../ports/help_view';
import type { Connectable, ProfileId, SetUp } from '../protocol';

const SET_UP_TOPIC = 'help-setting-up';

export interface ConnectAction {
  /** Resolves `false` when the profile could not be started, once the reason has been said. */
  (id: ProfileId, setUp: SetUp, origin: string | null): Promise<boolean>;
}

export class NewConnectionDialog {
  private rows: Connectable[] = [];
  private attempting = false;
  private readonly kindList: OptionList;
  private readonly panel: ConnectionPanel;

  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly kinds: HTMLElement,
    panelTitle: HTMLElement,
    panelBody: HTMLElement,
    private readonly connect: ConnectApi,
    private readonly start: ConnectAction,
    private readonly announcer: AnnouncerView,
    private readonly returnTo: { focus(): void },
    private readonly connecting: { show(label: string): void; hide(): void },
    private readonly help: HelpView,
    private readonly sayConnected: () => void = () => {},
  ) {
    this.panel = new ConnectionPanel(panelTitle, panelBody, announcer, 'new', {
      changed: () => this.formFilled(),
    });
    this.dialog.addEventListener('close', () => this.returnTo.focus());
    this.dialog.addEventListener('keydown', (event) => keepTabInside(this.dialog, event));
    this.dialog.addEventListener('keydown', (event) => this.enterConnects(event));
    this.kindList = new OptionList(this.kinds, 'new-kind', () => {
      this.showPanel();
      this.describe();
    });
    this.dialog
      .querySelector('#new-cancel')
      ?.addEventListener('click', () => this.dialog.close());
    this.dialog
      .querySelector('#new-start')
      ?.addEventListener('click', () => void this.chosen());
    const helpButton = this.dialog.querySelector<HTMLElement>('#new-set-up-help');
    helpButton?.addEventListener('click', () =>
      this.help.open({ topic: SET_UP_TOPIC, returnTo: helpButton }),
    );
  }

  async open(): Promise<void> {
    if (this.dialog.open) {
      return;
    }
    this.rows = await this.connect.connectable();
    this.render();
    this.dialog.showModal();
    this.kinds.focus();
    // NVDA 2026.1.1 reads a live region inside the dialog as the dialog opens, so an
    // unconditional announcement here was heard twice.
    this.describe();
  }

  private render(): void {
    this.kindList.fill({
      labels: this.rows.map((row) => row.label),
      selected: this.rows.length === 0 ? null : 0,
    });
    this.showPanel();
  }

  private get row(): Connectable | undefined {
    const at = this.kindList.chosen();
    return at === null ? undefined : this.rows[at];
  }

  private showPanel(): void {
    this.panel.show(this.row, null, null);
  }

  private formFilled(): void {
    const start = this.dialog.querySelector<HTMLButtonElement>('#new-start');
    if (start === null || this.attempting) {
      return;
    }
    start.disabled = !this.panel.startable();
  }

  private describe(): void {
    const row = this.row;
    if (row === undefined || !worthSaying(row)) {
      return;
    }
    this.announcer.announce(this.panel.summary());
  }

  private enterConnects(event: KeyboardEvent): void {
    if (event.key !== 'Enter') {
      return;
    }
    if ((event.target as HTMLElement).closest('button') !== null) {
      return;
    }
    event.preventDefault();
    void this.chosen();
  }

  private async chosen(): Promise<void> {
    const row = this.row;
    if (row === undefined) {
      return;
    }
    if (this.attempting) {
      return;
    }
    if (!this.panel.startable()) {
      const missing = this.panel.missing();
      if (missing !== null) {
        this.announcer.announce(missing);
      }
      return;
    }
    this.connecting.show(this.panel.label(row.label));
    this.busy(true);
    const started = await this.start(this.panel.profile(row.id), this.setUp(), null);
    this.busy(false);
    this.connecting.hide();
    if (started) {
      this.dialog.close();
      // Must come before `sayConnected`; see `documentReturned` in announcer.ts.
      this.announcer.documentReturned();
      this.sayConnected();
      return;
    }
    this.kinds.focus();
    this.describe();
  }

  private busy(connecting: boolean): void {
    this.attempting = connecting;
    this.dialog.setAttribute('aria-busy', String(connecting));
    for (const control of this.dialog.querySelectorAll<HTMLButtonElement>(
      'button, input, select',
    )) {
      control.disabled = connecting;
    }
    if (!connecting) {
      this.formFilled();
    }
  }

  /** A missing checkbox reads as ticked. */
  private setUp(): SetUp {
    const box = this.dialog.querySelector<HTMLInputElement>('#new-set-up');
    return box === null || box.checked ? 'Yes' : 'No';
  }
}
