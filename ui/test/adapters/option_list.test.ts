// @vitest-environment jsdom
// Role: test — the listbox both of the Connect dialog's lists are.
//
// The rules here are small and each one of them was a defect somewhere first: selection
// travels as `aria-activedescendant` so focus never leaves the list (A8, decision 2), the
// ends do not wrap, and a list can have **nothing** selected — which is the state the user
// asked for on 2026-08-30 when they asked for the variants combo box to become a list.

import { beforeEach, describe, expect, it } from 'vitest';

import { OptionList } from '../../src/adapters/option_list';

let element: HTMLElement;
let list: OptionList;
let chosen: number;

function press(key: string): void {
  element.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
}

function selected(): string | null {
  return (
    element.querySelector('[role="option"][aria-selected="true"]')?.textContent ?? null
  );
}

function active(): string | null {
  return element.getAttribute('aria-activedescendant');
}

beforeEach(() => {
  document.body.innerHTML = `<ul id="kinds" role="listbox" tabindex="0"></ul>`;
  element = document.getElementById('kinds') as HTMLElement;
  chosen = 0;
  list = new OptionList(element, 'kind', () => {
    chosen += 1;
  });
});

describe('filling it', () => {
  it('renders one option per label, named and marked', () => {
    list.fill({ labels: ['Command Prompt', 'WSL'], selected: 0 });

    const options = Array.from(element.querySelectorAll('[role="option"]'));
    expect(options.map((option) => option.textContent)).toEqual([
      'Command Prompt',
      'WSL',
    ]);
    expect(options.map((option) => option.id)).toEqual(['kind-0', 'kind-1']);
    expect(selected()).toBe('Command Prompt');
    expect(active()).toBe('kind-0');
  });

  /** **The state a combo box cannot hold.** Nothing marked, and nothing for a reader to
   * announce as active — an `aria-activedescendant` pointing at nothing would promise an
   * option that is not there. */
  it('can hold nothing selected', () => {
    list.fill({ labels: ['Ubuntu', 'Debian'], selected: null });

    expect(selected()).toBeNull();
    expect(active()).toBeNull();
    expect(list.chosen()).toBeNull();
  });

  it('replaces what was there, so a choice cannot survive a refill', () => {
    list.fill({ labels: ['Ubuntu', 'Debian'], selected: null });
    press('ArrowDown');

    list.fill({ labels: ['Windows PowerShell'], selected: null });

    expect(list.chosen()).toBeNull();
    expect(selected()).toBeNull();
  });
});

describe('arrowing it', () => {
  beforeEach(() => {
    list.fill({ labels: ['Ubuntu', 'Debian', 'Alpine'], selected: null });
  });

  /** From nothing, Down is the first choice rather than a correction. */
  it('takes the first row on Down when nothing is chosen', () => {
    press('ArrowDown');

    expect(selected()).toBe('Ubuntu');
    expect(list.chosen()).toBe(0);
    expect(chosen).toBe(1);
  });

  /** And Up takes the last, which is what a listbox with no selection does everywhere. */
  it('takes the last row on Up when nothing is chosen', () => {
    press('ArrowUp');

    expect(selected()).toBe('Alpine');
    expect(list.chosen()).toBe(2);
  });

  it('moves one row at a time and stops at the ends', () => {
    press('ArrowDown');
    press('ArrowDown');
    expect(selected()).toBe('Debian');

    press('End');
    press('ArrowDown');
    expect(selected()).toBe('Alpine');

    press('Home');
    press('ArrowUp');
    expect(selected()).toBe('Ubuntu');
  });

  /** Pressing into the end you are already on is not a change, so nothing is announced
   * again — the rule `dialog_tab` keeps for Tab, kept here for the arrows. */
  it('says nothing changed when nothing changed', () => {
    press('Home');
    const before = chosen;

    press('ArrowUp');
    press('Home');

    expect(chosen).toBe(before);
  });

  /** Every other key belongs to whoever is listening above: Enter connects, Tab leaves. */
  it('leaves other keys alone', () => {
    const enter = new KeyboardEvent('keydown', {
      key: 'Enter',
      bubbles: true,
      cancelable: true,
    });

    element.dispatchEvent(enter);

    expect(enter.defaultPrevented).toBe(false);
  });

  it('does nothing at all when there is nothing in it', () => {
    list.fill({ labels: [], selected: null });

    press('ArrowDown');

    expect(list.chosen()).toBeNull();
  });
});

describe('clicking it', () => {
  beforeEach(() => {
    list.fill({ labels: ['Ubuntu', 'Debian'], selected: null });
  });

  it('chooses the row that was clicked', () => {
    element.querySelector<HTMLElement>('#kind-1')?.click();

    expect(selected()).toBe('Debian');
    expect(active()).toBe('kind-1');
    expect(chosen).toBe(1);
  });

  it('ignores a click that landed on no row', () => {
    element.click();

    expect(list.chosen()).toBeNull();
    expect(chosen).toBe(0);
  });
});
