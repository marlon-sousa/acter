// Role: adapter (DOM) — the Connect dialog: a list of connection kinds, a panel holding
// whatever the chosen kind needs, and the three steps of connecting.
//
// **Why this is a dialog and not a submenu** (spec A8). A submenu is the better shape for a
// pure choice, and connecting to cmd or PowerShell is one. Connecting over SSH is not: a
// host, a port, a user and a key are a form, and no submenu holds one. The alternatives were
// two surfaces for one action — worst of all for somebody learning the application by ear —
// or one surface that carries both.
//
// It also earns its shape a second time on failure. A submenu that failed had nowhere to
// put the user back; this stays open with the reason announced and focus where they can
// choose something else.
//
// **The kinds and the variants are a deliberate division** (decision 3). This module knows
// what a kind *looks like* — that is what a view is for, and a backend describing its own
// controls would be a user interface written in Rust and reachable by no test. It knows no
// variants at all: which PowerShell editions are installed, which distributions exist, which
// connections the user saved are all `connectable()`'s answer, asked fresh every time this
// opens so a distribution installed while Acter is running appears without a restart.

import { keepTabInside } from './dialog_tab';
import type { AnnouncerView } from '../ports/announcer_view';
import type { ConnectApi } from '../ports/connect_api';
import type { Connectable, ProfileId } from '../protocol';

/** What the panel says when the chosen kind needs nothing. */
const NO_OPTIONS = 'no options';
/** What it says for a kind this machine cannot start; the instructions follow it. */
const NOT_AVAILABLE = 'not available';

/**
 * What a listener is told the panel now holds when they arrow onto a kind.
 *
 * **Counted and named, not just "options"**: "2 distributions" tells somebody whether it is
 * worth tabbing into the panel at all, which "the panel changed" does not. The noun comes
 * from the variants' own shape, which is this side's knowledge by decision 3 — the backend
 * says which things exist, the frontend says what they are called on screen.
 */
export function panelSummary(row: Connectable): string {
  if (!row.available) {
    return NOT_AVAILABLE;
  }
  if (row.variants.length === 0) {
    return NO_OPTIONS;
  }
  const noun =
    row.variants[0]?.id.profile === 'Distribution' ? 'distribution' : 'option';
  return `${row.variants.length} ${noun}${row.variants.length === 1 ? '' : 's'}`;
}

/** What the dialog needs of whoever actually connects: did it work. */
export interface ConnectAction {
  /**
   * Start this profile. Resolves true when the window is on it now, false when it could
   * not be started — in which case the reason has already been announced and this dialog
   * stays open (decision 4).
   */
  (id: ProfileId): Promise<boolean>;
}

export class ConnectDialog {
  private rows: Connectable[] = [];
  /** Which kind is chosen, as an index into `rows`. */
  private at = 0;

  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly kinds: HTMLElement,
    private readonly panelTitle: HTMLElement,
    private readonly panelBody: HTMLElement,
    private readonly connect: ConnectApi,
    private readonly start: ConnectAction,
    private readonly announcer: AnnouncerView,
    private readonly returnTo: { focus(): void },
  ) {
    // Escape is the platform's, and so is closing; where focus belongs afterwards is not,
    // because what opened this was a menu that no longer exists (spec A7, decision 3).
    this.dialog.addEventListener('close', () => this.returnTo.focus());
    // Tab past the last control lands on the dialog's own document rather than cycling —
    // measured with NVDA 2026.1.1 on 2026-08-26, where Tab past Cancel announced "dialog
    // Connect" and left the reader nowhere. The platform does not do this for us.
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
    this.kinds.addEventListener('keydown', (event) => this.navigate(event));
    this.kinds.addEventListener('click', (event) => this.clicked(event));
    this.dialog
      .querySelector('#connect-cancel')
      ?.addEventListener('click', () => this.dialog.close());
    this.dialog
      .querySelector('#connect-start')
      ?.addEventListener('click', () => void this.chosen());
  }

  /**
   * Open it, with the list asked for afresh.
   *
   * Opening an already-open dialog throws `InvalidStateError` and throws it silently into a
   * `void` call, and a menu item chosen twice is an ordinary thing — so this answers rather
   * than breaking, exactly as the About dialog does.
   */
  async open(): Promise<void> {
    if (this.dialog.open) {
      return;
    }
    this.rows = await this.connect.connectable();
    this.at = 0;
    this.render();
    this.dialog.showModal();
    // Focus goes to the list rather than to the dialog, so the first thing a listener hears
    // after the dialog names itself is the kind they are on rather than a container.
    this.kinds.focus();
    // **Only if there is something in the panel**, unlike a kind *change*, which always
    // says what the panel now holds. The reader reads the dialog as it opens, and a
    // live region inside it that already has text is read along with everything else —
    // so an unconditional announcement here was heard twice, a second apart (measured
    // with NVDA 2026.1.1 on 2026-08-26). Nothing is hidden by staying quiet: an empty
    // panel is not a change a listener has to be told about on arrival.
    if (this.hasPanelContent()) {
      this.describe();
    }
  }

  private hasPanelContent(): boolean {
    const row = this.row;
    return row !== undefined && (!row.available || row.variants.length > 0);
  }

  /** The kinds, as options; the panel, for whichever is chosen. */
  private render(): void {
    this.kinds.replaceChildren(
      ...this.rows.map((row, index) => {
        const option = this.kinds.ownerDocument.createElement('li');
        option.id = `connect-kind-${index}`;
        option.setAttribute('role', 'option');
        option.setAttribute('aria-selected', String(index === this.at));
        option.textContent = row.label;
        return option;
      }),
    );
    // Set from the first render, not only when the selection moves: it is what makes the
    // reader announce the kind — with its position in the list — as focus arrives, which is
    // a listbox naming itself rather than this module announcing it.
    this.kinds.setAttribute('aria-activedescendant', `connect-kind-${this.at}`);
    this.showPanel();
  }

  private get row(): Connectable | undefined {
    return this.rows[this.at];
  }

  /**
   * The panel for the chosen kind: nothing at all when it needs nothing, a list of its
   * variants when it has them, and what to do about it when this machine cannot start it.
   *
   * The variants are a `<select>` deliberately. A second listbox would be a second widget
   * needing its own arrow handling and its own mode, where a combo box is something a
   * listener can open with `Alt+Down` and arrow from any mode — one of the gestures the
   * platform's accessibility contract assumes every user has.
   */
  private showPanel(): void {
    const row = this.row;
    if (row === undefined) {
      return;
    }
    this.panelTitle.textContent = panelSummary(row);
    const document = this.panelBody.ownerDocument;
    this.panelBody.replaceChildren();

    if (!row.available) {
      // The instructions are prose to be *read*: what is missing, what to type, and where
      // (spec B5.4, decision 4). They are the backend's words, not this module's.
      const said = document.createElement('p');
      said.textContent = row.instructions ?? '';
      this.panelBody.append(said);
      return;
    }
    if (row.variants.length === 0) {
      return;
    }
    const label = document.createElement('label');
    label.htmlFor = 'connect-variant';
    label.textContent = 'Distribution';
    const select = document.createElement('select');
    select.id = 'connect-variant';
    for (const [index, variant] of row.variants.entries()) {
      const option = document.createElement('option');
      option.value = String(index);
      option.textContent = variant.label;
      select.append(option);
    }
    this.panelBody.append(label, select);
  }

  /**
   * Say what the panel now holds (decision 2).
   *
   * **The kind itself is not repeated here**, because the listbox has already said it: the
   * reader announces the option and its position from `aria-activedescendant`, and an
   * announcement that began with the label again made a listener hear "Command Prompt" twice
   * for one arrow press (measured with NVDA 2026.1.1 on 2026-08-26). What this adds is the
   * half no widget can say for itself — that a second control below has changed.
   */
  private describe(): void {
    const row = this.row;
    if (row === undefined) {
      return;
    }
    this.announcer.announce(panelSummary(row));
  }

  /**
   * Arrowing the kinds moves the selection and never moves focus.
   *
   * A list you cannot arrow through without leaving it is not a list, so the panel is
   * reached by Tab rather than by arriving in it (decision 2). Selection travels as
   * `aria-activedescendant`, which is what lets focus stay on the list while the reader
   * announces the option.
   */
  private navigate(event: KeyboardEvent): void {
    const last = this.rows.length - 1;
    if (last < 0) {
      return;
    }
    let to = this.at;
    switch (event.key) {
      case 'ArrowDown':
        to = Math.min(this.at + 1, last);
        break;
      case 'ArrowUp':
        to = Math.max(this.at - 1, 0);
        break;
      case 'Home':
        to = 0;
        break;
      case 'End':
        to = last;
        break;
      case 'Enter':
        event.preventDefault();
        void this.chosen();
        return;
      default:
        return;
    }
    event.preventDefault();
    if (to !== this.at) {
      this.at = to;
      this.select();
      this.describe();
    }
  }

  private clicked(event: Event): void {
    const option = (event.target as HTMLElement).closest('[role="option"]');
    const index = this.rows.findIndex(
      (_, at) => option?.id === `connect-kind-${at}`,
    );
    if (index === -1 || index === this.at) {
      return;
    }
    this.at = index;
    this.select();
    this.describe();
  }

  private select(): void {
    for (const [index, option] of Array.from(
      this.kinds.querySelectorAll<HTMLElement>('[role="option"]'),
    ).entries()) {
      option.setAttribute('aria-selected', String(index === this.at));
    }
    this.kinds.setAttribute('aria-activedescendant', `connect-kind-${this.at}`);
    this.showPanel();
  }

  /**
   * Connect to what is chosen: the variant if the panel offered any, the kind itself
   * otherwise — which for WSL means whatever distribution WSL calls the default.
   *
   * **A kind this machine cannot start is not a special case here**, and deliberately. The
   * button stays enabled and the call goes through, because the backend refuses it with the
   * very instructions the panel is showing — one path, one place the words are decided, and
   * no disabled control that reads differently from how it looks.
   */
  private async chosen(): Promise<void> {
    const row = this.row;
    if (row === undefined) {
      return;
    }
    if (await this.start(this.profile(row))) {
      // Closing puts focus back in the edit field; the far end the user is now on has
      // already been announced by whoever connected.
      this.dialog.close();
    }
  }

  private profile(row: Connectable): ProfileId {
    if (row.variants.length === 0) {
      return row.id;
    }
    const select =
      this.panelBody.querySelector<HTMLSelectElement>('#connect-variant');
    const at = Number(select?.value ?? 0);
    return row.variants[at]?.id ?? row.id;
  }
}
