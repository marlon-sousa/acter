// Role: adapter (DOM) — the Connect dialog: a list of the connection names somebody saved,
// the shared panel loaded from whichever they are on, and five buttons.

import { ConnectionPanel, kindFor } from './connection_panel';
import { keepTabInside } from './dialog_tab';
import { OptionList } from './option_list';
import type { AnnouncerView } from '../ports/announcer_view';
import type { ConnectApi } from '../ports/connect_api';
import type { Connectable, ProfileId, SavedRow, SetUp } from '../protocol';

export const NOTHING_SAVED =
  'No saved connections yet. New connection starts one, and Acter offers to save it once ' +
  'it is up.';

const NOT_AVAILABLE = 'not available';

export function forgetting(name: string): string {
  return (
    `Forget ${name}? It is removed from this list. Nothing on the computer it connected ` +
    'to changes.'
  );
}

export interface ConnectSaved {
  (id: ProfileId, setUp: SetUp, origin: string | null): Promise<boolean>;
}

export interface Asking {
  /** `null` when cancelled. */
  rename(name: string): Promise<string | null>;
  forget(question: string): Promise<boolean>;
}

export class ConnectDialog {
  private rows: SavedRow[] = [];
  private kinds: Connectable[] = [];
  private attempting = false;
  private readonly nameList: OptionList;
  private readonly panel: ConnectionPanel;

  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly names: HTMLElement,
    private readonly empty: HTMLElement,
    panelTitle: HTMLElement,
    panelBody: HTMLElement,
    private readonly connect: ConnectApi,
    private readonly start: ConnectSaved,
    private readonly announcer: AnnouncerView,
    private readonly returnTo: { focus(): void },
    private readonly connecting: { show(label: string): void; hide(): void },
    private readonly asking: Asking,
    private readonly newConnection: () => void,
    private readonly sayConnected: () => void = () => {},
  ) {
    this.panel = new ConnectionPanel(panelTitle, panelBody, announcer, 'saved', {
      changed: () => this.followTheChoice(),
    });
    this.dialog.addEventListener('close', () => this.returnTo.focus());
    this.dialog.addEventListener('keydown', (event) => keepTabInside(this.dialog, event));
    this.dialog.addEventListener('keydown', (event) => this.enterConnects(event));
    this.nameList = new OptionList(this.names, 'connect-name', () =>
      this.showPanel(true),
    );
    this.on('#connect-cancel', () => this.dialog.close());
    this.on('#connect-start', () => void this.chosen());
    this.on('#connect-rename', () => void this.rename());
    this.on('#connect-forget', () => void this.forget());
    this.on('#connect-new', () => {
      this.dialog.close();
      this.newConnection();
    });
  }

  private on(selector: string, act: () => void): void {
    this.dialog.querySelector(selector)?.addEventListener('click', act);
  }

  async open(): Promise<void> {
    if (this.dialog.open) {
      return;
    }
    await this.reload();
    this.dialog.showModal();
    this.focusStart();
  }

  private async reload(): Promise<void> {
    const [saved, kinds] = await Promise.all([
      this.connect.saved(),
      this.connect.connectable(),
    ]);
    this.rows = saved.rows;
    this.kinds = kinds;
    this.empty.textContent = saved.unreadable ?? NOTHING_SAVED;
    this.empty.hidden = this.rows.length > 0;
    this.names.hidden = this.rows.length === 0;
    // Without this description NVDA 2026.1.1 said only the dialog's name and the focused
    // button, and not the sentence explaining the empty list.
    if (this.rows.length === 0) {
      this.dialog.setAttribute('aria-describedby', this.empty.id);
    } else {
      this.dialog.removeAttribute('aria-describedby');
    }
    this.nameList.fill({
      labels: this.rows.map((row) => row.name),
      selected: this.rows.length === 0 ? null : 0,
    });
    this.showPanel(false);
  }

  private focusStart(): void {
    if (this.rows.length === 0) {
      this.dialog.querySelector<HTMLElement>('#connect-new')?.focus();
      return;
    }
    this.names.focus();
  }

  private get row(): SavedRow | undefined {
    const at = this.nameList.chosen();
    return at === null ? undefined : this.rows[at];
  }

  private showPanel(announce: boolean): void {
    const row = this.row;
    if (row === undefined) {
      this.panel.show(undefined, null, null);
      return;
    }
    this.panel.show(
      kindFor(this.kinds, row.id),
      row.id,
      row.available ? null : row.instructions,
    );
    this.setUpBox(row.set_up);
    if (!announce) {
      return;
    }
    this.announcer.announce(row.available ? row.summary : NOT_AVAILABLE);
  }

  private followTheChoice(): void {
    const start = this.dialog.querySelector<HTMLButtonElement>('#connect-start');
    if (start === null || this.attempting) {
      return;
    }
    start.disabled = this.row === undefined || !this.panel.startable();
  }

  private setUpBox(set_up: SetUp): void {
    const box = this.dialog.querySelector<HTMLInputElement>('#connect-set-up');
    if (box !== null) {
      box.checked = set_up === 'Yes';
    }
  }

  private setUp(): SetUp {
    const box = this.dialog.querySelector<HTMLInputElement>('#connect-set-up');
    return box === null || box.checked ? 'Yes' : 'No';
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
    if (row === undefined || this.attempting) {
      return;
    }
    if (!this.panel.startable()) {
      const missing = this.panel.missing();
      if (missing !== null) {
        this.announcer.announce(missing);
      }
      return;
    }
    this.connecting.show(row.name);
    this.busy(true);
    const started = await this.start(
      this.panel.profile(row.id),
      this.setUp(),
      row.name,
    );
    this.busy(false);
    this.connecting.hide();
    if (started) {
      this.dialog.close();
      this.announcer.documentReturned();
      this.sayConnected();
      return;
    }
    this.names.focus();
  }

  private async rename(): Promise<void> {
    const row = this.row;
    if (row === undefined) {
      return;
    }
    const to = await this.asking.rename(row.name);
    if (to === null) {
      this.names.focus();
      return;
    }
    await this.answered(
      this.connect.renameConnection(row.name, to),
      (rows) => rows.findIndex((named) => named.name === to.trim()),
    );
  }

  private async forget(): Promise<void> {
    const row = this.row;
    if (row === undefined) {
      return;
    }
    if (!(await this.asking.forget(forgetting(row.name)))) {
      this.names.focus();
      return;
    }
    const at = this.nameList.chosen() ?? 0;
    await this.answered(this.connect.forgetConnection(row.name), () => at);
  }

  private async answered(
    acting: Promise<string>,
    landing: (rows: SavedRow[]) => number,
  ): Promise<void> {
    let said: string;
    try {
      said = await acting;
    } catch (why) {
      this.announcer.announce(typeof why === 'string' ? why : String(why));
      this.names.focus();
      return;
    }
    await this.reload();
    if (this.rows.length === 0) {
      this.announcer.announce(said);
      this.dialog.querySelector<HTMLElement>('#connect-new')?.focus();
      return;
    }
    const at = Math.min(Math.max(landing(this.rows), 0), this.rows.length - 1);
    this.nameList.select(at);
    this.names.focus();
    this.announcer.announce(said);
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
      this.followTheChoice();
    }
  }
}
