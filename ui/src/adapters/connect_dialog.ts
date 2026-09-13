// Role: adapter (DOM) — the Connect dialog: a list of the connection names somebody saved,
// the shared panel loaded from whichever they are on, and five buttons.
//
// **This is what File → Connect opens since spec 26** (decisions 12 to 16). It used to open
// the list of *kinds*, which is now File → New connection and lives in
// `new_connection_dialog.ts`. The division is what the whole entry is about: connecting to
// something you have connected to before is picking a name off a list, and it should not
// mean walking a list of kinds and refilling a form.
//
// **The list is alphabetical, without case, and stable** (decision 12). A listener learns
// positions, so the order is never most-recent-first: a list that reorders itself under
// somebody is a list they have to read from the top every time. Focus lands on the first
// name, so the everyday case is open, arrow, Enter.
//
// **Arrowing loads the panel and never moves focus** (decision 13), and the panel is loaded
// with the row's own values: the SSH form filled in, the distribution or edition selected,
// the set-up checkbox as it was saved.
//
// **Edits in the panel apply to this attempt only** (decision 14). Changing the port and
// pressing Connect connects to that port and writes nothing; keeping the change is File →
// Save connection afterwards, which the help says in one sentence.
//
// **There is no Save here** (decision 15), because saving happens from a live session and
// nowhere else — so there is one way to save rather than two.

import { ConnectionPanel, kindFor } from './connection_panel';
import { keepTabInside } from './dialog_tab';
import { OptionList } from './option_list';
import type { AnnouncerView } from '../ports/announcer_view';
import type { ConnectApi } from '../ports/connect_api';
import type { Connectable, ProfileId, SavedRow, SetUp } from '../protocol';

/**
 * What the dialog says where the list would be when nothing is saved (decision 16).
 *
 * **The empty list says what to do about itself**, which is the unconnected window's own
 * rule: one place to learn, and nothing that leaves a listener in front of a container with
 * no contents and no next step.
 */
export const NOTHING_SAVED =
  'No saved connections yet. New connection starts one, and Acter offers to save it once ' +
  'it is up.';

/** What is said when a row that is not available is arrowed onto (decision 13). */
const NOT_AVAILABLE = 'not available';

/** What Forget asks before it does the one thing here nobody can undo (decision 15). */
export function forgetting(name: string): string {
  return (
    `Forget ${name}? It is removed from this list. Nothing on the computer it connected ` +
    'to changes.'
  );
}

/** What the dialog needs of whoever actually connects: did it work. */
export interface ConnectSaved {
  /**
   * Start this profile, as the saved connection `origin` — so who holds the line follows
   * what was saved, and the window does not offer to save something it already has a name
   * for (spec 26, decisions 11 and 19).
   */
  (id: ProfileId, setUp: SetUp, origin: string | null): Promise<boolean>;
}

/** What it needs of the two dialogs it opens on top of itself. */
export interface Asking {
  /** Ask for a new name, prefilled and selected, and answer it or `null` for cancel. */
  rename(name: string): Promise<string | null>;
  /**
   * Put this question, and answer whether they said yes.
   *
   * **It takes the question rather than the name**, because the words are this module's:
   * they sit beside the empty-list sentence, where every other string this dialog says
   * lives, rather than in the composition root — which constructs objects and decides no
   * wording at all.
   */
  forget(question: string): Promise<boolean>;
}

export class ConnectDialog {
  private rows: SavedRow[] = [];
  /** What this machine offers, so a saved row can be loaded into the same panel. */
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
    /** What New connection opens, after this dialog has closed (decision 15). */
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
    // **It closes this one first, because dialogs do not stack** (decision 15). Cancel from
    // the New connection dialog returns to the window rather than to this list, which is
    // one dialog to escape from rather than two.
    this.on('#connect-new', () => {
      this.dialog.close();
      this.newConnection();
    });
  }

  private on(selector: string, act: () => void): void {
    this.dialog.querySelector(selector)?.addEventListener('click', act);
  }

  /**
   * Open it, with the saved connections and the machine's kinds asked for afresh.
   *
   * Both are asked every time for `connectable`'s reason (spec B7, decision 6): a
   * distribution installed while Acter was open, and a connection saved in another window,
   * are both true the next time this opens without a restart.
   */
  async open(): Promise<void> {
    if (this.dialog.open) {
      return;
    }
    await this.reload();
    this.dialog.showModal();
    this.focusStart();
  }

  /** Ask again and redraw, which is what Rename and Forget do to themselves. */
  private async reload(): Promise<void> {
    const [saved, kinds] = await Promise.all([
      this.connect.saved(),
      this.connect.connectable(),
    ]);
    this.rows = saved.rows;
    this.kinds = kinds;
    // **The one place a document that would not parse is reported** (decisions 9 and 16).
    // It takes the place of the empty-list sentence, names both files and says what was
    // wrong with the first — because there is nothing saved either way, and the difference
    // is whether the person should go looking for a file.
    this.empty.textContent = saved.unreadable ?? NOTHING_SAVED;
    this.empty.hidden = this.rows.length > 0;
    this.names.hidden = this.rows.length === 0;
    // **And it is what the dialog says as it opens** (ARCHITECTURE, dialogs rule 5).
    // Measured with NVDA 2026.1.1 on 2026-09-12: with nothing saved the dialog announced
    // its name and the button focus landed on, and nothing else — the sentence was in the
    // document and a listener had to go and find it, which is prose most listeners will
    // not find. It matters most for the half that is not the ordinary empty list: a
    // document that would not parse names two files, and that is the one thing somebody
    // needs to hear.
    //
    // **Removed again when there are rows**, so a dialog that has something in its list
    // does not read out a sentence about being empty.
    if (this.rows.length === 0) {
      this.dialog.setAttribute('aria-describedby', this.empty.id);
    } else {
      this.dialog.removeAttribute('aria-describedby');
    }
    this.nameList.fill({
      labels: this.rows.map((row) => row.name),
      selected: this.rows.length === 0 ? null : 0,
    });
    // **Nothing is announced here** (decision 13): what a listener hears as the dialog
    // opens is the name focus lands on, said by the listbox itself. An announcement on top
    // of that was heard twice, a second apart, when A8 tried it in the kinds dialog.
    this.showPanel(false);
  }

  /**
   * Where focus lands as the dialog opens: the first name, or New connection when there is
   * nothing to arrow (decision 16).
   */
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

  /**
   * Load the panel from the row the listener is on, and say the one line describing it
   * (decision 13).
   *
   * The set-up checkbox goes with it, because it is one of the two settings a saved
   * connection holds and the panel it belongs beside has just been loaded from the same
   * row.
   */
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
    // **The kind and what identifies it, in one line** (decision 13) — composed in the
    // domain, because every other spoken string on this seam is. A row that cannot be
    // started says so instead, which is the fact rather than the description.
    this.announcer.announce(row.available ? row.summary : NOT_AVAILABLE);
  }

  /** Connect follows the panel exactly as it does in the New connection dialog. */
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

  /** Enter connects from anywhere in the dialog that is not a button, as today. */
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

  /**
   * Connect to the name the listener is on, with whatever the panel now holds.
   *
   * **The panel's current values, and nothing is written** (decision 14): changing the port
   * and pressing Connect connects to that port, and keeping the change is File → Save
   * connection afterwards.
   */
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
      // **The name this attempt started from** (decision 11), so who holds the line
      // follows what was saved and the window does not offer to save it again.
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

  /**
   * Rename the row the listener is on, and put focus back on it (decision 15).
   *
   * A refusal is announced and the row stays where it was: the backend keeps the words,
   * because a name can arrive from somewhere that never saw this dialog.
   */
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
      // **Focus returns to the renamed row**, which is where the listener was.
      (rows) => rows.findIndex((named) => named.name === to.trim()),
    );
  }

  /**
   * Forget the row the listener is on, after asking once (decision 15).
   *
   * **Focus lands on the row that follows**, or on New connection when the list is empty:
   * a listener who removed a row is still working through the list, and putting them back
   * at the top would make them find their place again.
   */
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

  /**
   * Say what the backend answered, redraw, and put focus where the decision says.
   *
   * **Both halves are sentences a listener hears**, which is why one path serves the two
   * actions: the words are the backend's, and what this owns is when they are said and
   * where focus lands afterwards.
   */
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
    // Clamped, because forgetting the last row leaves the index past the end and the row
    // that follows it is then the one before it.
    const at = Math.min(Math.max(landing(this.rows), 0), this.rows.length - 1);
    this.nameList.select(at);
    this.names.focus();
    this.announcer.announce(said);
  }

  /** Make the dialog unusable while a connection is being made, and usable again after. */
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
