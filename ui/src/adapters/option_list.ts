// Role: adapter (DOM) — a listbox whose selection travels as `aria-activedescendant`, so
// focus never leaves the list while a reader announces the option under it.

export interface OptionLabels {
  labels: string[];
  selected: number | null;
}

export class OptionList {
  private at: number | null = null;
  private count = 0;

  /** `prefix` must be unique in the document, because option ids are `${prefix}-${index}`. */
  constructor(
    private readonly element: HTMLElement,
    private readonly prefix: string,
    private readonly chose: () => void,
  ) {
    this.element.addEventListener('keydown', (event) => this.navigate(event));
    this.element.addEventListener('click', (event) => this.clicked(event));
  }

  fill({ labels, selected }: OptionLabels): void {
    const document = this.element.ownerDocument;
    this.count = labels.length;
    this.at = selected;
    this.element.replaceChildren(
      ...labels.map((label, index) => {
        const option = document.createElement('li');
        option.id = `${this.prefix}-${index}`;
        option.setAttribute('role', 'option');
        option.setAttribute('aria-selected', String(index === selected));
        option.textContent = label;
        return option;
      }),
    );
    this.mark();
  }

  chosen(): number | null {
    return this.at;
  }

  select(at: number | null): void {
    this.at = at === null || at < 0 || at >= this.count ? null : at;
    this.mark();
  }

  focus(): void {
    this.element.focus();
  }

  private navigate(event: KeyboardEvent): void {
    const last = this.count - 1;
    if (last < 0) {
      return;
    }
    const at = this.at;
    let to: number;
    switch (event.key) {
      case 'ArrowDown':
        to = at === null ? 0 : Math.min(at + 1, last);
        break;
      case 'ArrowUp':
        to = at === null ? last : Math.max(at - 1, 0);
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
    if (to !== at) {
      this.at = to;
      this.mark();
      this.chose();
    }
  }

  private clicked(event: Event): void {
    const option = (event.target as HTMLElement).closest('[role="option"]');
    if (option === null) {
      return;
    }
    const index = Array.from(this.element.children).indexOf(option);
    if (index === -1 || index === this.at) {
      return;
    }
    this.at = index;
    this.mark();
    this.chose();
  }

  private mark(): void {
    for (const [index, option] of Array.from(
      this.element.querySelectorAll<HTMLElement>('[role="option"]'),
    ).entries()) {
      option.setAttribute('aria-selected', String(index === this.at));
    }
    if (this.at === null) {
      this.element.removeAttribute('aria-activedescendant');
      return;
    }
    this.element.setAttribute('aria-activedescendant', `${this.prefix}-${this.at}`);
  }
}
