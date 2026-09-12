// Role: adapter (DOM) — the New connection dialog: a list of connection kinds, the shared
// panel holding whatever the chosen kind needs, and the three steps of connecting.
//
// **This is A8's Connect dialog, renamed and otherwise unchanged** (spec 26, decision 17).
// File → Connect opens a different dialog now, whose list is the saved connection names;
// this is what File → New connection opens, and what the saved-connections dialog's New
// connection button opens. There is no name field: naming happens after the connection is
// up, because only a connection that actually came up is worth saving.
//
// **Why this is a dialog and not a submenu** (spec A8). A submenu is the better shape for a
// pure choice, and connecting to cmd or PowerShell is one. Connecting over SSH is not: a
// host, a port, a user and a key are a form, and no submenu holds one.
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
// **The panel is not this module's any more** (spec 26, decision 17). Two dialogs show the
// same one, so it lives in `connection_panel.ts` and this owns the kinds list, the checkbox
// and the three steps of connecting.

import { ConnectionPanel, worthSaying } from './connection_panel';
import { keepTabInside } from './dialog_tab';
import { OptionList } from './option_list';
import type { AnnouncerView } from '../ports/announcer_view';
import type { ConnectApi } from '../ports/connect_api';
import type { HelpView } from '../ports/help_view';
import type { Connectable, ProfileId, SetUp } from '../protocol';

/** The section of the help topic that explains the checkbox this dialog carries. */
const SET_UP_TOPIC = 'help-setting-up';

/** What the dialog needs of whoever actually connects: did it work. */
export interface ConnectAction {
  /**
   * Start this profile. Resolves true when the window is on it now, false when it could
   * not be started — in which case the reason has already been announced and this dialog
   * stays open (spec A8, decision 4).
   *
   * `setUp` is the checkbox below the panel: whether this connection may run one command
   * inside the session once it is established (spec B9.5, decision 9). `origin` is the
   * saved connection it started from, which for this dialog is always `null` — a new
   * connection has no name yet, which is what makes the window offer to save it (spec 26,
   * decision 19).
   */
  (id: ProfileId, setUp: SetUp, origin: string | null): Promise<boolean>;
}

export class NewConnectionDialog {
  private rows: Connectable[] = [];
  /** Whether an attempt is in flight, so a second one cannot be started into it. */
  private attempting = false;
  /** The kinds, as the one widget they have always been (spec A8, decision 2). */
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
    // **Where Enter goes now** (reported 2026-08-30): the dialog that says a connection is
    // being made, rather than the list of kinds this used to bounce back to.
    private readonly connecting: { show(label: string): void; hide(): void },
    // And what the Help button beside the set-up checkbox opens, at the section about it.
    private readonly help: HelpView,
    // What names the far end the listener is now on, called after this dialog has closed and
    // focus has gone back to the edit field (roadmap 13.3). The words are the controller's;
    // when they are said is this dialog's, because only it knows when it is out of the way.
    private readonly sayConnected: () => void = () => {},
  ) {
    this.panel = new ConnectionPanel(panelTitle, panelBody, announcer, 'new', {
      changed: () => this.formFilled(),
    });
    // Escape is the platform's, and so is closing; where focus belongs afterwards is not,
    // because what opened this was a menu that no longer exists (spec A7, decision 3).
    this.dialog.addEventListener('close', () => this.returnTo.focus());
    // Tab past the last control lands on the dialog's own document rather than cycling —
    // measured with NVDA 2026.1.1 on 2026-08-26. The platform does not do this for us.
    this.dialog.addEventListener('keydown', (event) => keepTabInside(this.dialog, event));
    this.dialog.addEventListener('keydown', (event) => this.enterConnects(event));
    // Arrowing the kinds moves the selection and never moves focus, which is the widget
    // rather than this module: `OptionList` owns the keys, the ids and the marking.
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
    // The one control here that opens something rather than doing something: what the
    // checkbox above it turns on is four sentences, and an announcement is not where any of
    // them belong (spec A13, decision 2).
    const helpButton = this.dialog.querySelector<HTMLElement>('#new-set-up-help');
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
    this.render();
    this.dialog.showModal();
    // Focus goes to the list rather than to the dialog, so the first thing a listener hears
    // after the dialog names itself is the kind they are on rather than a container.
    this.kinds.focus();
    // **Only if there is something in the panel**, unlike a kind *change*, which always
    // says what the panel now holds. The reader reads the dialog as it opens, and a live
    // region inside it that already has text is read along with everything else — so an
    // unconditional announcement here was heard twice, a second apart (measured with NVDA
    // 2026.1.1 on 2026-08-26).
    this.describe();
  }

  /** The kinds, as options; the panel, for whichever is chosen. */
  private render(): void {
    // Selected from the first render rather than only when the selection moves: it is what
    // makes the reader announce the kind — with its position in the list — as focus arrives.
    //
    // **A kind is always chosen and a variant is not.** Opening onto a kind is opening onto
    // something you can connect to; opening onto a distribution would be the browser
    // choosing for you.
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
    // Nothing chosen in it: a new connection starts from nothing, which is the whole
    // difference from the saved-connections dialog beside it.
    this.panel.show(this.row, null, null);
  }

  /**
   * Keep Connect available only while there is something to connect to.
   *
   * **What a disabled button is not is an announcement.** Measured with NVDA 2026.1.1 on
   * 2026-08-30 — Tab went from the Help button straight to Cancel, because `keepTabInside`
   * filters disabled controls out of the cycle. So a listener never meets the disabled
   * button at all, and what tells them is `chosen`'s sentence when Enter cannot connect.
   */
  private formFilled(): void {
    const start = this.dialog.querySelector<HTMLButtonElement>('#new-start');
    if (start === null || this.attempting) {
      return;
    }
    start.disabled = !this.panel.startable();
  }

  /** Say what the panel now holds (spec A8, decision 2). */
  private describe(): void {
    const row = this.row;
    if (row === undefined || !worthSaying(row)) {
      return;
    }
    this.announcer.announce(this.panel.summary());
  }

  /**
   * **Enter is the dialog's default action, from anywhere in it.**
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

  /**
   * Connect to what is chosen: the variant if the panel offered any, the kind itself
   * otherwise — which for WSL means whatever distribution WSL calls the default.
   *
   * **A kind this machine cannot start is not a special case here**, and deliberately. The
   * button stays enabled and the call goes through, because the backend refuses it with the
   * very instructions the panel is showing — one path, one place the words are decided.
   */
  private async chosen(): Promise<void> {
    const row = this.row;
    if (row === undefined) {
      return;
    }
    // **Nothing in here can be pressed while an attempt is running** — reported by the
    // user on 2026-08-26, who was left focused on the Connect button for the seconds a
    // connection took, and could press it again into the attempt already in flight.
    if (this.attempting) {
      return;
    }
    // **Not silently, though.** A disabled Connect says what it says only to somebody who
    // tabs to it, and Enter is the key this dialog answers from everywhere.
    if (!this.panel.startable()) {
      const missing = this.panel.missing();
      if (missing !== null) {
        this.announcer.announce(missing);
      }
      return;
    }
    // **Forward, into the dialog that says what is happening** — reported by the user on
    // 2026-08-30, who pressed Enter and was put back on the list of kinds.
    this.connecting.show(this.panel.label(row.label));
    this.busy(true);
    // **No origin**, and that is what makes this dialog the one the offer follows: a new
    // connection has no name yet (spec 26, decision 19).
    const started = await this.start(this.panel.profile(row.id), this.setUp(), null);
    this.busy(false);
    this.connecting.hide();
    if (started) {
      // Closing puts focus back in the edit field.
      this.dialog.close();
      // **And only now is the far end named** (roadmap 13.3 and 23.13, fixed 2026-08-30).
      // A region that has just returned eats the first change made to it, so the announcer
      // is told to spend a wordless one first.
      this.announcer.documentReturned();
      this.sayConnected();
      return;
    }
    // **Back to the list, not left on whatever was pressed** — reported by the user on
    // 2026-08-26, who was returned to the Cancel button after a connection was refused.
    this.kinds.focus();
    this.describe();
  }

  /**
   * Make the dialog unusable while a connection is being made, and usable again after.
   *
   * The controls are *disabled* rather than merely ignored, so a reader says so rather than
   * leaving somebody pressing a button that answers nothing.
   */
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

  /**
   * Whether this connection may set its session up, as the checkbox says right now.
   *
   * **Ticked by default, and unticking it is reachable without any dialog appearing** (spec
   * B9.5, decision 9). A missing checkbox reads as ticked, for the reason every default in
   * this file does: the ordinary case is the one that has to work when something is not
   * where it was expected.
   */
  private setUp(): SetUp {
    const box = this.dialog.querySelector<HTMLInputElement>('#new-set-up');
    return box === null || box.checked ? 'Yes' : 'No';
  }
}
