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
// **It does not hold the listener while the connection is made** (reported 2026-08-30). It
// stays open, its controls unavailable, underneath a dialog that says what is happening —
// because being sent back to the list of kinds you have just pressed Enter on is this dialog
// saying that nothing happened, for as long as a cold distribution takes to come up.
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
import type { HelpView } from '../ports/help_view';
import type { Connectable, ProfileId, SetUp } from '../protocol';

/** What the panel says when the chosen kind needs nothing. */
const NO_OPTIONS = 'no options';

/**
 * What a variants list starts on, and the value that means nothing has been chosen.
 *
 * **Reported by the user on 2026-08-30**: choosing WSL and pressing Enter connected to
 * Ubuntu, because the browser selects the first option of a `<select>` for you. A kind is
 * chosen by arrowing onto it, which nobody does deliberately, so the distribution that came
 * first in the list was being connected to by somebody who had never heard its name. What a
 * combo box says it is on has to be something the person did.
 */
const NOTHING_CHOSEN = 'not chosen';

/** The section of the help topic that explains the checkbox this dialog carries. */
const SET_UP_TOPIC = 'help-setting-up';

/** What is missing when a kind with variants has none of them chosen. */
function chooseOneFirst(row: Connectable): string {
  return `choose a ${noun(row)} first`;
}

/**
 * The fields an SSH connection needs, in the order they are filled in.
 *
 * **Three fields and a port, rather than one box holding `user@host:port`.** A spelling has
 * to be parsed and can be got wrong, and getting it wrong for somebody who cannot see the
 * box is a silent failure; these are the facts themselves (spec B9). The port is filled in
 * with 22, because that is what it is unless somebody moved it.
 */
const SSH_FIELDS = [
  { name: 'host', label: 'Host', type: 'text', value: '' },
  { name: 'port', label: 'Port', type: 'number', value: '22' },
  { name: 'user', label: 'Account', type: 'text', value: '' },
] as const;

/** Whether this row is the one that needs a form. */
function isSsh(row: Connectable): boolean {
  return row.id.profile === 'Ssh';
}
/** What it says for a kind this machine cannot start; the instructions follow it. */
const NOT_AVAILABLE = 'not available';
/** The heading over the SSH form — what the panel *is*, rather than how many boxes. */
const DETAILS = 'Connection details';

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
  // **The one kind that is a form rather than a choice** (spec A8, decision 1), and the
  // one whose panel is not a count of anything: three empty boxes is how much typing there
  // is, not what there is to choose between.
  if (isSsh(row)) {
    return DETAILS;
  }
  if (row.variants.length === 0) {
    return NO_OPTIONS;
  }
  const count = row.variants.length;
  return `${count} ${noun(row)}${count === 1 ? '' : 's'}`;
}

/**
 * Whether arrowing onto this row is worth saying anything about.
 *
 * **Only when the kind cannot be started at all — A8 decision 2 reversed on use,
 * 2026-08-26**, reported by the user driving the real dialog: "better to remove these
 * announcements for all list items", with "not available" kept.
 *
 * That decision announced what the panel now holds, on the reasoning that a section
 * changing silently under a listener is a trap. The reasoning was sound and the case it
 * was built on turns out to be rare: most rows have nothing worth saying, so what the
 * summary actually adds is a second utterance between every arrow press and the next,
 * paid on every navigation for a benefit that lands occasionally. "No options" is a
 * sentence about a container that is empty; "3 fields" counts boxes nobody chooses
 * between. Both are the panel talking about itself.
 *
 * What survives is the one that is a fact rather than a description: a kind this machine
 * cannot start says so, and the instructions under it are the point of the panel.
 */
function worthSaying(row: Connectable): boolean {
  return !row.available;
}

/**
 * What this kind's variants are called on screen.
 *
 * **The frontend's knowledge, by A8's decision 3**: the backend says which things exist, and
 * what they are called in a user interface is this side's. It is read off the variant's own
 * shape rather than from the row, because that is the fact that decides it — a distribution
 * is a distribution whichever kind carried it.
 */
function noun(row: Connectable): string {
  switch (row.variants[0]?.id.profile) {
    case 'Distribution':
      return 'distribution';
    // **Two shapes, one noun, since B5.7.** A variant that names a kind is an edition this
    // machine does not have; one that names an *install* is an edition it does, carrying the
    // file the list already resolved (spec B5.7, decision 1). Both are editions to a
    // listener, and the panel would otherwise call them "options" on every machine that has
    // PowerShell at all.
    case 'Shell':
    case 'Install':
      return 'edition';
    default:
      return 'option';
  }
}

/** What the dialog needs of whoever actually connects: did it work. */
export interface ConnectAction {
  /**
   * Start this profile. Resolves true when the window is on it now, false when it could
   * not be started — in which case the reason has already been announced and this dialog
   * stays open (decision 4).
   *
   * `setUp` is the checkbox below the panel: whether this connection may run one command
   * inside the session once it is established (spec B9.5, decision 9). It travels with the
   * attempt rather than being stored, because there is no profile store to keep it in until
   * B8 — which is also why it is read here, at the moment Connect is pressed, rather than
   * remembered anywhere.
   */
  (id: ProfileId, setUp: SetUp): Promise<boolean>;
}

export class ConnectDialog {
  private rows: Connectable[] = [];
  /** Whether an attempt is in flight, so a second one cannot be started into it. */
  private attempting = false;
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
    // **Where Enter goes now** (reported 2026-08-30): the dialog that says a connection is
    // being made, rather than the list of kinds this used to bounce back to.
    private readonly connecting: { show(label: string): void; hide(): void },
    // And what the Help button beside the set-up checkbox opens, at the section about it.
    private readonly help: HelpView,
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
    this.dialog.addEventListener('keydown', (event) => this.enterConnects(event));
    this.kinds.addEventListener('keydown', (event) => this.navigate(event));
    this.kinds.addEventListener('click', (event) => this.clicked(event));
    this.dialog
      .querySelector('#connect-cancel')
      ?.addEventListener('click', () => this.dialog.close());
    this.dialog
      .querySelector('#connect-start')
      ?.addEventListener('click', () => void this.chosen());
    // The one control here that opens something rather than doing something: what the
    // checkbox above it turns on is four sentences, and an announcement is not where any of
    // them belong (spec A13, decision 2). Focus comes back to this button, because the
    // dialog it opens sits on top of one that is still here.
    const helpButton = this.dialog.querySelector<HTMLElement>('#connect-set-up-help');
    helpButton?.addEventListener('click', () =>
      this.help.open({ topic: SET_UP_TOPIC, returnTo: helpButton }),
    );
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
    return row !== undefined && worthSaying(row);
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
    // The button comes back on the way out of a kind that could be incomplete — a form, or
    // a list nobody had chosen from — and the branches that build one of those ask
    // `formFilled` for themselves before they are done.
    const start = this.dialog.querySelector<HTMLButtonElement>('#connect-start');
    if (start !== null && !this.attempting) {
      start.disabled = false;
    }

    if (!row.available) {
      // The instructions are prose to be *read*: what is missing, what to type, and where
      // (spec B5.4, decision 4). They are the backend's words, not this module's.
      this.panelBody.append(this.instructions(row.instructions ?? ''));
      return;
    }
    if (isSsh(row)) {
      this.showSshForm();
      return;
    }
    if (row.variants.length === 0) {
      return;
    }
    const label = document.createElement('label');
    label.htmlFor = 'connect-variant';
    // Capitalised because it names a control rather than counting things: "Distribution",
    // "Edition". The summary above it does the counting.
    const which = noun(row);
    label.textContent = which.charAt(0).toUpperCase() + which.slice(1);
    const select = document.createElement('select');
    select.id = 'connect-variant';
    // **It starts on nothing, and that is the whole of the fix** (reported 2026-08-30). A
    // `<select>` selects its first option for you, so a listener who chose WSL and pressed
    // Enter connected to whichever distribution happened to be first — a choice they never
    // made and never heard. An option that says so is the honest starting state: the combo
    // box reads "not chosen", Connect is unavailable until it is, and nothing is connected
    // to on somebody's behalf.
    const nothing = document.createElement('option');
    nothing.value = '';
    nothing.textContent = NOTHING_CHOSEN;
    select.append(nothing);
    for (const [index, variant] of row.variants.entries()) {
      const option = document.createElement('option');
      option.value = String(index);
      option.textContent = variant.label;
      select.append(option);
    }
    // **A variant can be unavailable while its kind is not** — PowerShell 7 on a machine
    // that only has Windows PowerShell — so what to do about it has to appear when it is
    // chosen, and be *said*, because a panel that changes silently under a listener is the
    // trap decision 2 exists to answer.
    select.addEventListener('change', () => {
      this.showVariantInstructions(row);
      // Connect follows the choice, exactly as it follows the SSH form: there is nothing to
      // connect to until one is made, and going back to "not chosen" takes it away again.
      this.formFilled();
      const chosen = this.variant(row);
      if (chosen !== undefined && !chosen.available) {
        this.announcer.announce(NOT_AVAILABLE);
      }
    });
    this.panelBody.append(label, select);
    this.showVariantInstructions(row);
    // The button follows the panel here exactly as it follows the SSH form: this one has
    // just been rebuilt with nothing chosen in it, so there is nothing to connect to yet.
    this.formFilled();
  }

  /**
   * The form for a far end that is not on this machine.
   *
   * **Ordinary labelled inputs, and no widget of its own.** A text box inside an
   * application region is one of the few things that behaves identically in every reading
   * mode, so this is the part of the dialog that needs the least explaining — which is
   * exactly what a form asking for a host and an account should be.
   */
  private showSshForm(): void {
    const document = this.panelBody.ownerDocument;
    for (const field of SSH_FIELDS) {
      const label = document.createElement('label');
      label.htmlFor = `connect-ssh-${field.name}`;
      label.textContent = field.label;
      const input = document.createElement('input');
      input.id = `connect-ssh-${field.name}`;
      input.type = field.type;
      input.value = field.value;
      // Nothing here is remembered between openings: a saved connection is B8's, and a
      // form that half-remembered would be a form a listener has to check before trusting.
      input.autocomplete = 'off';
      // **The button follows the form** — reported by the user on 2026-08-26: "why is the
      // connect button ever enabled when information isn't complete?"
      input.addEventListener('input', () => this.formFilled());
      this.panelBody.append(label, input);
    }
    this.formFilled();
  }

  /**
   * Keep Connect available only while there is something to connect to.
   *
   * **A8 decision 4 does not reach this case, and applying it here was the mistake.** That
   * decision keeps Connect enabled for a kind this machine cannot start, so pressing it
   * answers with the instructions — useful, because nothing you do in the dialog changes
   * that. An empty host is not that: it is a form you have not finished, and the answer is
   * not information you lacked, and the old shape only told you after a round trip you had
   * to wait for.
   *
   * **What a disabled button is not is an announcement**, and the note here used to claim
   * otherwise: "tabbing to it and hearing 'unavailable' says the form is incomplete".
   * Measured with NVDA 2026.1.1 on 2026-08-30 — Tab went from the Help button straight to
   * Cancel, because `keepTabInside` filters disabled controls out of the cycle, which it
   * does deliberately and for a good reason of its own. So a listener never meets the
   * disabled button at all, and what tells them is `chosen`'s sentence when Enter cannot
   * connect. The button being unavailable is still right; it is simply not the thing that
   * speaks.
   *
   * The backend keeps refusing an empty host with its own sentence, because a profile can
   * arrive from somewhere that is not this form.
   */
  private formFilled(): void {
    const start = this.dialog.querySelector<HTMLButtonElement>('#connect-start');
    if (start === null) {
      return;
    }
    start.disabled = !this.startable();
  }

  /**
   * Whether there is something to connect to at all.
   *
   * **One condition, asked in both places** — reported by the user on 2026-08-26: pressing
   * Enter on the SSH row with every field blank started an attempt and answered with the
   * backend's error, because `enterConnects` reaches `chosen` directly and never consulted
   * the button it was standing in for. A disabled button that Enter walks straight past is
   * not a disabled button; it is a lie told to whoever tabbed to it.
   *
   * Every kind that is not a form is startable as it stands, which is what the `true` at
   * the end says: only the row that asks for details can be incomplete.
   */
  private startable(): boolean {
    const row = this.row;
    if (row === undefined) {
      return true;
    }
    // **A kind with variants is incomplete until one of them is chosen** (reported
    // 2026-08-30), which is the SSH form's rule reaching the other shape of the same
    // question: a panel nobody has answered is a panel nobody has answered, whether it asks
    // for a host or for a distribution.
    if (!isSsh(row) && row.variants.length > 0) {
      return this.variant(row) !== undefined;
    }
    if (!isSsh(row)) {
      return true;
    }
    const filled = (name: string): boolean =>
      (this.panelBody
        .querySelector<HTMLInputElement>(`#connect-ssh-${name}`)
        ?.value.trim() ?? '') !== '';
    return filled('host') && filled('user');
  }

  /** What the form was filled in with, as the profile that starts it. */
  private sshProfile(fallback: ProfileId): ProfileId {
    const read = (name: string): string =>
      this.panelBody
        .querySelector<HTMLInputElement>(`#connect-ssh-${name}`)
        ?.value.trim() ?? '';
    const host = read('host');
    if (host === '') {
      // **Left to the backend to refuse**, with the sentence it already has for an unfilled
      // form — one path, one place the words are decided, and no disabled control that
      // reads differently from how it looks (the reasoning decision 4 applies to an
      // unavailable kind).
      return fallback;
    }
    const port = Number(read('port'));
    return {
      profile: 'Ssh',
      host,
      port: Number.isFinite(port) && port > 0 ? port : 22,
      user: read('user'),
    };
  }

  /** What to do about the chosen variant, when there is nothing to be done with it. */
  private showVariantInstructions(row: Connectable): void {
    const existing = this.panelBody.querySelector('[data-instructions]');
    existing?.remove();
    const chosen = this.variant(row);
    if (chosen === undefined || chosen.available) {
      return;
    }
    this.panelBody.append(this.instructions(chosen.instructions ?? ''));
  }

  /**
   * Read-only prose, made focusable.
   *
   * The dialog is an application region, and prose inside one cannot be arrowed — so without
   * a tab stop the one thing a user of an unavailable kind actually needs would be
   * unreachable.
   */
  private instructions(text: string): HTMLElement {
    const said = this.panelBody.ownerDocument.createElement('p');
    said.setAttribute('data-instructions', '');
    said.tabIndex = 0;
    said.textContent = text;
    return said;
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
    if (row === undefined || !worthSaying(row)) {
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

  /**
   * **Enter is the dialog's default action, from anywhere in it.**
   *
   * It used to be handled on the kinds list alone, which meant a user who tabbed into the
   * panel, chose a distribution and pressed Enter got nothing at all — reported by the user
   * on 2026-08-26, choosing Debian. Pressing Enter after making a choice is what every
   * dialog on this platform does, and a dialog that answers it in one of its controls and
   * not the others is one you have to learn by failing.
   *
   * A button is left alone, because it answers Enter itself: catching it here would connect
   * when the user pressed Cancel.
   */
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
    // **Nothing in here can be pressed while an attempt is running** — reported by the
    // user on 2026-08-26, who was left focused on the Connect button for the seconds a
    // connection took, and could press it again into the attempt already in flight. For
    // somebody navigating by focus, sitting on a control called Connect *is* being told
    // that connecting has not started.
    //
    // It also covers the gap between two questions: the password dialog closes, the next
    // one does not exist yet, and without this there is a moment where focus falls back
    // onto live controls belonging to a conversation still in progress.
    if (this.attempting) {
      return;
    }
    // **Not silently, though.** A disabled Connect says what it says only to somebody who
    // tabs to it, and Enter is the key this dialog answers from everywhere — so an Enter
    // that cannot connect says what is missing rather than nothing at all, which is the
    // difference between a dialog you learn and one you learn by failing.
    if (!this.startable()) {
      if (!isSsh(row) && row.variants.length > 0) {
        this.announcer.announce(chooseOneFirst(row));
      }
      return;
    }
    // **Forward, into the dialog that says what is happening** — reported by the user on
    // 2026-08-30, who pressed Enter and was put back on the list of kinds. Shown before the
    // controls are disabled, so focus moves into it rather than off a control that is being
    // taken away underneath it.
    this.connecting.show(this.chosenLabel(row));
    this.busy(true);
    const started = await this.start(this.profile(row), this.setUp());
    this.busy(false);
    this.connecting.hide();
    if (started) {
      // Closing puts focus back in the edit field; the far end the user is now on has
      // already been announced by whoever connected.
      this.dialog.close();
      return;
    }
    // **Back to the list, not left on whatever was pressed** — reported by the user on
    // 2026-08-26, who was returned to the Cancel button after a connection was refused.
    // Decision 4 keeps this dialog open on failure precisely so somebody can choose again,
    // and a listener parked on Cancel has been handed the one control that gives up.
    this.kinds.focus();
    this.describe();
  }

  /**
   * Make the dialog unusable while a connection is being made, and usable again after.
   *
   * The controls are *disabled* rather than merely ignored, so a reader says so rather than
   * leaving somebody pressing a button that answers nothing. It matters for the seconds
   * this dialog is underneath the connecting one, and for anybody who presses Escape out of
   * that and arrives back here while the attempt is still running.
   *
   * **It no longer moves focus** (reported 2026-08-30). Sending focus to the list of kinds
   * was this dialog telling a listener that pressing Enter had achieved nothing; the
   * connecting dialog is where focus goes now, and it is opened before this is called.
   */
  private busy(connecting: boolean): void {
    this.attempting = connecting;
    this.dialog.setAttribute('aria-busy', String(connecting));
    for (const control of this.dialog.querySelectorAll<HTMLButtonElement>(
      'button, input, select',
    )) {
      control.disabled = connecting;
    }
  }

  /**
   * Whether this connection may set its session up, as the checkbox says right now.
   *
   * **Ticked by default, and unticking it is reachable without any dialog appearing** (spec
   * B9.5, decision 9) — which is the whole reason this control is here rather than only
   * inside the dialog that discloses the command. A missing checkbox reads as ticked, for the
   * reason every default in this file does: the ordinary case is the one that has to work
   * when something is not where it was expected.
   */
  private setUp(): SetUp {
    const box = this.dialog.querySelector<HTMLInputElement>('#connect-set-up');
    return box === null || box.checked ? 'Yes' : 'No';
  }

  private profile(row: Connectable): ProfileId {
    if (isSsh(row)) {
      return this.sshProfile(row.id);
    }
    // The kind itself when it offers nothing to choose between, and otherwise the variant
    // that was chosen — which `startable` has already established there is one of.
    return this.variant(row)?.id ?? row.id;
  }

  /**
   * Which variant is chosen, or `undefined` while none is.
   *
   * One place asks the combo box, because three used to and each read the empty value its
   * own way. `Number('')` is `0`, which is exactly the wrong answer here: it is the first
   * variant, which is what "nothing chosen" must never mean again.
   */
  private variant(row: Connectable): Connectable['variants'][number] | undefined {
    const select =
      this.panelBody.querySelector<HTMLSelectElement>('#connect-variant');
    const value = select?.value ?? '';
    if (value === '') {
      return undefined;
    }
    return row.variants[Number(value)];
  }

  /** What the listener is connecting to, in the words the connection will use itself. */
  private chosenLabel(row: Connectable): string {
    const variant = this.variant(row);
    return variant === undefined ? row.label : `${row.label}: ${variant.label}`;
  }
}
