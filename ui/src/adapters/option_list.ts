// Role: adapter (DOM) — a listbox whose selection travels as `aria-activedescendant`, so
// focus never leaves the list while a reader announces the option under it.
//
// **It exists because there are two of them now** (reported by the user on 2026-08-30, who
// asked for the variants panel to stop being a combo box). The Connect dialog's kinds have
// been this shape since A8; the distributions and editions beside them are the same widget
// with a different filling, and two copies of arrow handling are two things that can drift —
// `dialog_tab`'s reasoning, applied to the second widget that needed it.
//
// **A list can have nothing selected, and that is why the user asked for one.** A `<select>`
// selects its first option for you, so "nothing chosen" had to be spelled as an option
// reading "not chosen" — a list item that is not a thing you can connect to, sitting in a
// list of things you can. A listbox simply starts with no selection: nothing is marked, no
// option is active, and the first arrow press is the first choice anybody made.

/** What one row is: the label a listener hears, and nothing else this module needs. */
export interface OptionLabels {
  labels: string[];
  /** Which row starts selected, or `null` for a list nobody has answered yet. */
  selected: number | null;
}

export class OptionList {
  private at: number | null = null;
  private count = 0;

  /**
   * `prefix` names the options — `${prefix}-0`, `${prefix}-1` — because
   * `aria-activedescendant` points at an id, and two lists in one dialog must not collide.
   *
   * `chose` runs whenever the selection moves, by arrow or by click. It is what the dialog
   * hangs its own behaviour on: announcing the panel, or following the choice with the
   * Connect button.
   */
  constructor(
    private readonly element: HTMLElement,
    private readonly prefix: string,
    private readonly chose: () => void,
  ) {
    this.element.addEventListener('keydown', (event) => this.navigate(event));
    this.element.addEventListener('click', (event) => this.clicked(event));
  }

  /** Put these rows in it, selecting one of them or none. */
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

  /** Which row is chosen, or `null` while none is. */
  chosen(): number | null {
    return this.at;
  }

  focus(): void {
    this.element.focus();
  }

  /**
   * Arrowing moves the selection and never moves focus.
   *
   * A list you cannot arrow through without leaving it is not a list, so what travels is
   * `aria-activedescendant` (spec A8, decision 2). **From nothing, Down and Home take the
   * first and Up and End take the last**, which is what a listbox with no selection does
   * everywhere: the first press is a choice rather than a correction.
   */
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

  /**
   * Say which option is selected, and which one the reader should be announcing.
   *
   * **With nothing selected the attribute is removed rather than emptied**: an
   * `aria-activedescendant` pointing at nothing is a promise of an option that is not there,
   * and a reader asked to describe it has nothing to say.
   */
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
