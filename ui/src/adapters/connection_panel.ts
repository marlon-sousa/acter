// Role: adapter (DOM) — the panel under a list, holding whatever the thing chosen above it
// needs: nothing for cmd, the installed distributions for WSL, a form for SSH, and the
// reason plus the instructions for anything this machine cannot start.

import { OptionList } from './option_list';
import type { AnnouncerView } from '../ports/announcer_view';
import type { Connectable, ProfileId, Variant } from '../protocol';

const NO_OPTIONS = 'no options';

export const NOT_AVAILABLE = 'not available';

const DETAILS = 'Connection details';

const SSH_FIELDS = [
  { name: 'host', label: 'Host', type: 'text', value: '' },
  { name: 'port', label: 'Port', type: 'number', value: '22' },
  { name: 'user', label: 'Account', type: 'text', value: '' },
] as const;

const DEFAULT_SSH_PORT = 22;

export function isSsh(row: Connectable): boolean {
  return row.id.profile === 'Ssh';
}

export function panelSummary(row: Connectable): string {
  if (!row.available) {
    return NOT_AVAILABLE;
  }
  if (isSsh(row)) {
    return DETAILS;
  }
  if (row.variants.length === 0) {
    return NO_OPTIONS;
  }
  const count = row.variants.length;
  return `${count} ${noun(row)}${count === 1 ? '' : 's'}`;
}

export function worthSaying(row: Connectable): boolean {
  return !row.available;
}

function noun(row: Connectable): string {
  const variant = row.variants[0]?.id;
  switch (variant?.profile) {
    case 'Distribution':
      return 'distribution';
    case 'Shell':
    case 'Install':
      return variant.kind === 'Terminal' ? 'shell' : 'edition';
    default:
      return 'option';
  }
}

export function chooseOneFirst(row: Connectable): string {
  return `choose a ${noun(row)} first`;
}

/** `undefined` for a profile no row carries, such as a program named directly. */
export function kindFor(
  kinds: Connectable[],
  id: ProfileId,
): Connectable | undefined {
  return kinds.find((row) => carries(row, id));
}

function carries(row: Connectable, id: ProfileId): boolean {
  if (sameProfile(row.id, id)) {
    return true;
  }
  if (row.variants.some((variant) => sameProfile(variant.id, id))) {
    return true;
  }
  switch (id.profile) {
    case 'Distribution':
      return row.id.profile === 'Shell' && row.id.kind === 'Wsl';
    case 'Shell':
    case 'Install':
      return editionOf(id.kind) === kindOfRow(row);
    case 'Ssh':
      return row.id.profile === 'Ssh';
    default:
      return false;
  }
}

function editionOf(kind: string): string {
  return kind === 'WindowsPowerShell' || kind === 'PowerShellSeven'
    ? 'PowerShell'
    : kind;
}

function kindOfRow(row: Connectable): string | undefined {
  return row.id.profile === 'Shell' || row.id.profile === 'Install'
    ? editionOf(row.id.kind)
    : undefined;
}

export function sameProfile(one: ProfileId, another: ProfileId): boolean {
  return JSON.stringify(one) === JSON.stringify(another);
}

export interface PanelHost {
  changed(): void;
}

export class ConnectionPanel {
  private showing: Connectable | undefined;
  private variants: OptionList | null = null;
  private refusal: string | null = null;

  /** `prefix` must be unique in the document, because it prefixes every control id. */
  constructor(
    private readonly title: HTMLElement,
    private readonly body: HTMLElement,
    private readonly announcer: AnnouncerView,
    private readonly prefix: string,
    private readonly host: PanelHost,
  ) {}

  /**
   * `start` `null` means nothing is chosen; `unavailable` is a reason from outside the kind,
   * such as a saved distribution that is gone.
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

  summary(): string {
    if (this.refusal !== null) {
      return NOT_AVAILABLE;
    }
    return this.showing === undefined ? NO_OPTIONS : panelSummary(this.showing);
  }

  startable(): boolean {
    const kind = this.showing;
    if (kind === undefined || this.refusal !== null) {
      // Startable on purpose: the backend refuses it with the instructions the panel shows.
      return true;
    }
    if (!isSsh(kind) && kind.variants.length > 0) {
      return this.variant() !== undefined;
    }
    if (!isSsh(kind)) {
      return true;
    }
    const filled = (name: string): boolean => this.read(name) !== '';
    return filled('host') && filled('user');
  }

  /** `null` when there is nothing to say about what is missing. */
  missing(): string | null {
    const kind = this.showing;
    if (kind === undefined || this.refusal !== null || isSsh(kind)) {
      return null;
    }
    return kind.variants.length > 0 ? chooseOneFirst(kind) : null;
  }

  profile(fallback: ProfileId): ProfileId {
    const kind = this.showing;
    if (kind !== undefined && isSsh(kind)) {
      return this.sshProfile(fallback);
    }
    return this.variant()?.id ?? fallback;
  }

  label(fallback: string): string {
    const variant = this.variant();
    if (this.showing === undefined || variant === undefined) {
      return fallback;
    }
    return `${this.showing.label}: ${variant.label}`;
  }

  describe(): void {
    if (this.refusal !== null) {
      this.announcer.announce(NOT_AVAILABLE);
    }
  }

  private variant(): Variant | undefined {
    const at = this.variants?.chosen();
    return at === undefined || at === null ? undefined : this.showing?.variants[at];
  }

  private showVariants(kind: Connectable, start: ProfileId | null): void {
    const document = this.body.ownerDocument;
    const list = document.createElement('ul');
    list.id = `${this.prefix}-variant`;
    list.setAttribute('role', 'listbox');
    list.tabIndex = 0;
    const which = noun(kind);
    list.setAttribute('aria-label', which.charAt(0).toUpperCase() + which.slice(1));
    this.body.append(list);
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

  private showVariantInstructions(kind: Connectable): void {
    this.body.querySelector('[data-instructions]')?.remove();
    const chosen = this.variant();
    if (chosen === undefined || chosen.available) {
      return;
    }
    this.body.append(this.instructions(chosen.instructions ?? ''));
  }

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
      input.addEventListener('input', () => this.host.changed());
      this.body.append(label, input);
    }
    this.host.changed();
  }

  private sshProfile(fallback: ProfileId): ProfileId {
    const host = this.read('host');
    if (host === '') {
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

  /** Focusable, because a reader cannot arrow through prose inside `role="application"`. */
  private instructions(text: string): HTMLElement {
    const said = this.body.ownerDocument.createElement('p');
    said.setAttribute('data-instructions', '');
    said.tabIndex = 0;
    said.textContent = text;
    return said;
  }
}

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
