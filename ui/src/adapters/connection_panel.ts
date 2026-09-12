// Role: adapter (DOM) — the panel under a list, holding whatever the thing chosen above it
// needs: nothing for cmd, the installed distributions for WSL, a form for SSH, and the
// reason plus the instructions for anything this machine cannot start.
//
// **Extracted from the Connect dialog by spec 26** (decision 17), because there are two
// dialogs now and they show the same panel. The New connection dialog loads it from a kind
// with nothing chosen; the Connect dialog loads it from a saved connection, with the
// distribution or the edition already selected and the SSH form already filled in. Two
// copies of this would be two things that can drift, and the one that drifts is the one
// nobody is driving that week — `dialog_tab`'s reasoning, applied to the third thing two
// dialogs needed.
//
// **The kinds and the variants are a deliberate division** (spec A8, decision 3). This
// module knows what a kind *looks like* — that is what a view is for, and a backend
// describing its own controls would be a user interface written in Rust and reachable by no
// test. It knows no variants at all: which PowerShell editions are installed and which
// distributions exist are `connectable()`'s answer.

import { OptionList } from './option_list';
import type { AnnouncerView } from '../ports/announcer_view';
import type { Connectable, ProfileId, Variant } from '../protocol';

/** What the panel says when the chosen kind needs nothing. */
const NO_OPTIONS = 'no options';

/** What it says for something this machine cannot start; the instructions follow it. */
export const NOT_AVAILABLE = 'not available';

/** The heading over the SSH form — what the panel *is*, rather than how many boxes. */
const DETAILS = 'Connection details';

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

/** The port every SSH server listens on unless somebody moved it. */
const DEFAULT_SSH_PORT = 22;

/** Whether this row is the one that needs a form. */
export function isSsh(row: Connectable): boolean {
  return row.id.profile === 'Ssh';
}

/**
 * What a listener is told the panel now holds when they arrow onto a kind.
 *
 * **Counted and named, not just "options"**: "2 distributions" tells somebody whether it is
 * worth tabbing into the panel at all, which "the panel changed" does not. The noun comes
 * from the variants' own shape, which is this side's knowledge by A8's decision 3 — the
 * backend says which things exist, the frontend says what they are called on screen.
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
 * summary actually adds is a second utterance between every arrow press and the next.
 *
 * What survives is the one that is a fact rather than a description: a kind this machine
 * cannot start says so, and the instructions under it are the point of the panel.
 */
export function worthSaying(row: Connectable): boolean {
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
  const variant = row.variants[0]?.id;
  switch (variant?.profile) {
    case 'Distribution':
      return 'distribution';
    // **Two shapes, one noun, since B5.7.** A variant that names a kind is an edition this
    // machine does not have; one that names an *install* is an edition it does, carrying the
    // file the list already resolved (spec B5.7, decision 1). Both are editions to a
    // listener, and the panel would otherwise call them "options" on every machine that has
    // PowerShell at all.
    //
    // **Except on a Mac, where the same two shapes carry shells** (spec M2). A Terminal row's
    // variants are `/bin/zsh` and its neighbours, and calling those editions would name them
    // after a Windows product a listener has never met.
    case 'Shell':
    case 'Install':
      return variant.kind === 'Terminal' ? 'shell' : 'edition';
    default:
      return 'option';
  }
}

/** What is missing when a kind with variants has none of them chosen. */
export function chooseOneFirst(row: Connectable): string {
  return `choose a ${noun(row)} first`;
}

/**
 * Which kind in the connect list a profile belongs to, or `undefined` when none does.
 *
 * **This is what lets a saved connection load the same panel** (spec 26, decision 13): the
 * document remembers the edition and the distribution, and the panel needs the row those
 * live in so the listener meets the whole choice with theirs already made.
 *
 * A program named directly belongs to no kind, because the catalogue has no row for one —
 * so its panel holds nothing, which is honest: there is nothing about it to choose.
 */
export function kindFor(
  kinds: Connectable[],
  id: ProfileId,
): Connectable | undefined {
  return kinds.find((row) => carries(row, id));
}

/** Whether this row is the one that would carry that profile, as itself or as a variant. */
function carries(row: Connectable, id: ProfileId): boolean {
  if (sameProfile(row.id, id)) {
    return true;
  }
  if (row.variants.some((variant) => sameProfile(variant.id, id))) {
    return true;
  }
  switch (id.profile) {
    // A distribution belongs to the WSL row even when it is one this machine no longer
    // has, which is exactly the case decision 7 has to keep listed.
    case 'Distribution':
      return row.id.profile === 'Shell' && row.id.kind === 'Wsl';
    // And an edition belongs to the PowerShell row even when it is gone, for the reason a
    // missing edition stays in the panel (spec A11).
    case 'Shell':
    case 'Install':
      return editionOf(id.kind) === kindOfRow(row);
    case 'Ssh':
      return row.id.profile === 'Ssh';
    default:
      return false;
  }
}

/** The row a kind belongs under: the two PowerShell editions live under PowerShell. */
function editionOf(kind: string): string {
  return kind === 'WindowsPowerShell' || kind === 'PowerShellSeven'
    ? 'PowerShell'
    : kind;
}

/** Which kind a row is, when it is one that has a kind at all. */
function kindOfRow(row: Connectable): string | undefined {
  return row.id.profile === 'Shell' || row.id.profile === 'Install'
    ? editionOf(row.id.kind)
    : undefined;
}

/** Whether these two profiles name the same thing to start. */
export function sameProfile(one: ProfileId, another: ProfileId): boolean {
  return JSON.stringify(one) === JSON.stringify(another);
}

/** What the panel needs of whoever is showing it. */
export interface PanelHost {
  /** Something in the panel changed, so whatever follows a choice should follow it. */
  changed(): void;
}

export class ConnectionPanel {
  /** The kind this panel is currently showing, or `undefined` before anything is shown. */
  private showing: Connectable | undefined;
  /**
   * The panel's own list, while the kind has variants to put in one.
   *
   * Rebuilt with the panel rather than kept and refilled, because the panel is rebuilt: a
   * list belonging to the kind before this one is exactly the choice that must not survive.
   */
  private variants: OptionList | null = null;
  /** What to say when this kind cannot be started, whoever decided that. */
  private refusal: string | null = null;

  /**
   * `prefix` names the panel's controls, because two dialogs hold one of these each and
   * two elements in one document must not share an id.
   */
  constructor(
    private readonly title: HTMLElement,
    private readonly body: HTMLElement,
    private readonly announcer: AnnouncerView,
    private readonly prefix: string,
    private readonly host: PanelHost,
  ) {}

  /**
   * Show the panel for this kind, with `start` already chosen in it.
   *
   * `start` is the profile the panel opens on: a saved connection's, so the distribution or
   * the edition is selected and the SSH form is filled in — or `null` for a new connection,
   * where **nothing is chosen and that is the point**. Opening onto a distribution would be
   * the browser choosing for you, which is the whole of the 2026-08-30 report.
   *
   * `unavailable` is a reason from outside the kind: a saved distribution that is gone is
   * unavailable even though WSL is not (spec 26, decision 7).
   */
  show(
    kind: Connectable | undefined,
    start: ProfileId | null = null,
    unavailable: string | null = null,
  ): void {
    this.showing = kind;
    this.refusal = unavailable ?? (kind?.available === false ? kind.instructions : null);
    this.variants = null;
    this.body.replaceChildren();
    this.title.textContent = this.summary();

    if (this.refusal !== null) {
      // The instructions are prose to be *read*: what is missing, what to type, and where
      // (spec B5.4, decision 4). They are the backend's words, not this module's.
      this.body.append(this.instructions(this.refusal));
      this.host.changed();
      return;
    }
    if (kind === undefined) {
      this.host.changed();
      return;
    }
    if (isSsh(kind)) {
      this.showSshForm(start);
      return;
    }
    if (kind.variants.length === 0) {
      this.host.changed();
      return;
    }
    this.showVariants(kind, start);
  }

  /** What the panel's heading says, which is also what a listener hears about a kind. */
  summary(): string {
    if (this.refusal !== null) {
      return NOT_AVAILABLE;
    }
    return this.showing === undefined ? NO_OPTIONS : panelSummary(this.showing);
  }

  /**
   * Whether there is something to connect to at all.
   *
   * **One condition, asked in every place that can start the action** — reported by the
   * user on 2026-08-26: pressing Enter on the SSH row with every field blank started an
   * attempt and answered with the backend's error, because the key handler reached the
   * action directly and never consulted the button it was standing in for.
   *
   * Every kind that is not a form is startable as it stands, which is what the `true` at
   * the end says: only the row that asks for details can be incomplete.
   */
  startable(): boolean {
    const kind = this.showing;
    if (kind === undefined || this.refusal !== null) {
      // **Deliberately startable**, which is A8's decision 4: pressing Connect on a kind
      // this machine cannot start answers with the very instructions the panel is showing,
      // and nothing the user does in the dialog changes that.
      return true;
    }
    // **A kind with variants is incomplete until one of them is chosen** (reported
    // 2026-08-30), which is the SSH form's rule reaching the other shape of the same
    // question: a panel nobody has answered is a panel nobody has answered, whether it asks
    // for a host or for a distribution.
    if (!isSsh(kind) && kind.variants.length > 0) {
      return this.variant() !== undefined;
    }
    if (!isSsh(kind)) {
      return true;
    }
    const filled = (name: string): boolean => this.read(name) !== '';
    return filled('host') && filled('user');
  }

  /** What is missing, when Enter cannot connect and something can be said about it. */
  missing(): string | null {
    const kind = this.showing;
    if (kind === undefined || this.refusal !== null || isSsh(kind)) {
      return null;
    }
    return kind.variants.length > 0 ? chooseOneFirst(kind) : null;
  }

  /**
   * What to connect to: the variant if the panel offered any, the form's values if it is a
   * form, and the fallback otherwise — which for a saved connection is the profile it was
   * loaded with, and for a kind is the kind itself.
   */
  profile(fallback: ProfileId): ProfileId {
    const kind = this.showing;
    if (kind !== undefined && isSsh(kind)) {
      return this.sshProfile(fallback);
    }
    return this.variant()?.id ?? fallback;
  }

  /** What the listener is connecting to, in the words the connection will use itself. */
  label(fallback: string): string {
    const variant = this.variant();
    if (this.showing === undefined || variant === undefined) {
      return fallback;
    }
    return `${this.showing.label}: ${variant.label}`;
  }

  /** Say what the panel now holds, when there is anything worth saying (decision 2). */
  describe(): void {
    if (this.refusal !== null) {
      this.announcer.announce(NOT_AVAILABLE);
    }
  }

  /** Which variant is chosen, or `undefined` while none is. */
  private variant(): Variant | undefined {
    const at = this.variants?.chosen();
    return at === undefined || at === null ? undefined : this.showing?.variants[at];
  }

  /**
   * The variants, as a list.
   *
   * **A list, and not a combo box** — asked for by the user on 2026-08-30. What the list
   * buys is the state a combo box cannot hold: **nothing selected**. A `<select>` selects
   * its first option for you, so a listener who chose WSL and pressed Enter connected to
   * whichever distribution came first — a choice they never made and never heard.
   */
  private showVariants(kind: Connectable, start: ProfileId | null): void {
    const document = this.body.ownerDocument;
    const list = document.createElement('ul');
    list.id = `${this.prefix}-variant`;
    list.setAttribute('role', 'listbox');
    list.tabIndex = 0;
    // Capitalised because it names a control rather than counting things: "Distribution",
    // "Edition". The summary above it does the counting.
    const which = noun(kind);
    list.setAttribute('aria-label', which.charAt(0).toUpperCase() + which.slice(1));
    this.body.append(list);
    // **A variant can be unavailable while its kind is not** — PowerShell 7 on a machine
    // that only has Windows PowerShell — so what to do about it has to appear when it is
    // chosen, and be *said*, because a panel that changes silently under a listener is the
    // trap decision 2 exists to answer.
    this.variants = new OptionList(list, `${this.prefix}-variant`, () => {
      this.showVariantInstructions(kind);
      this.host.changed();
      const chosen = this.variant();
      if (chosen !== undefined && !chosen.available) {
        this.announcer.announce(NOT_AVAILABLE);
      }
    });
    const at = start === null ? -1 : kind.variants.findIndex((variant) => sameProfile(variant.id, start));
    this.variants.fill({
      labels: kind.variants.map((variant) => variant.label),
      selected: at === -1 ? null : at,
    });
    this.showVariantInstructions(kind);
    this.host.changed();
  }

  /** What to do about the chosen variant, when there is nothing to be done with it. */
  private showVariantInstructions(kind: Connectable): void {
    this.body.querySelector('[data-instructions]')?.remove();
    const chosen = this.variant();
    if (chosen === undefined || chosen.available) {
      return;
    }
    this.body.append(this.instructions(chosen.instructions ?? ''));
  }

  /**
   * The form for a far end that is not on this machine.
   *
   * **Ordinary labelled inputs, and no widget of its own.** A text box inside an
   * application region is one of the few things that behaves identically in every reading
   * mode, so this is the part of the dialog that needs the least explaining.
   *
   * **Filled in from a saved connection when there is one** (spec 26, decision 13), and
   * empty otherwise: a form that half-remembered would be a form a listener has to check
   * before trusting.
   */
  private showSshForm(start: ProfileId | null): void {
    const document = this.body.ownerDocument;
    const saved = start !== null && start.profile === 'Ssh' ? start : null;
    for (const field of SSH_FIELDS) {
      const label = document.createElement('label');
      label.htmlFor = `${this.prefix}-ssh-${field.name}`;
      label.textContent = field.label;
      const input = document.createElement('input');
      input.id = `${this.prefix}-ssh-${field.name}`;
      input.type = field.type;
      input.value = saved === null ? field.value : filled(saved, field.name);
      input.autocomplete = 'off';
      // **The button follows the form** — reported by the user on 2026-08-26: "why is the
      // connect button ever enabled when information isn't complete?"
      input.addEventListener('input', () => this.host.changed());
      this.body.append(label, input);
    }
    this.host.changed();
  }

  /** What the form was filled in with, as the profile that starts it. */
  private sshProfile(fallback: ProfileId): ProfileId {
    const host = this.read('host');
    if (host === '') {
      // **Left to the backend to refuse**, with the sentence it already has for an unfilled
      // form — one path, one place the words are decided.
      return fallback;
    }
    const port = Number(this.read('port'));
    return {
      profile: 'Ssh',
      host,
      port: Number.isFinite(port) && port > 0 ? port : DEFAULT_SSH_PORT,
      user: this.read('user'),
    };
  }

  private read(name: string): string {
    return (
      this.body
        .querySelector<HTMLInputElement>(`#${this.prefix}-ssh-${name}`)
        ?.value.trim() ?? ''
    );
  }

  /**
   * Read-only prose, made focusable.
   *
   * The dialog is an application region, and prose inside one cannot be arrowed — so
   * without a tab stop the one thing a user of an unavailable kind actually needs would be
   * unreachable.
   */
  private instructions(text: string): HTMLElement {
    const said = this.body.ownerDocument.createElement('p');
    said.setAttribute('data-instructions', '');
    said.tabIndex = 0;
    said.textContent = text;
    return said;
  }
}

/** One field of a saved SSH connection, as the form shows it. */
function filled(saved: Extract<ProfileId, { profile: 'Ssh' }>, name: string): string {
  switch (name) {
    case 'host':
      return saved.host;
    case 'port':
      return String(saved.port);
    default:
      return saved.user;
  }
}
