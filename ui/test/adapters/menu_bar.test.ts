// @vitest-environment jsdom
// Role: test — the menu bar's keyboard contract, which is the whole of what it is.
//
// The subject is navigation and what it leaves behind: where focus is, which submenu is
// open, what `aria-expanded` claims, and which of the two leaf actions ran. This menu bar
// exists because a *native* one freezes NVDA for tens of seconds every time it opens
// (spec A7), so what has to be pinned here is that the document version behaves the way a
// menu bar behaves — arrows, Enter, Escape — since nothing else in the product asserts it.

import { beforeEach, describe, expect, it } from 'vitest';

import { installMenuBar } from '../../src/adapters/menu_bar';

/** The static skeleton from views/main_window.html, restated so this suite tests the
 * structure the product ships rather than one invented for the test. */
const SKELETON = `
  <ul id="menu-bar" role="menubar" aria-label="Acter">
    <li role="none">
      <span role="menuitem" id="menu-acter" aria-haspopup="true" aria-expanded="false" tabindex="0">Acter</span>
      <ul role="menu" aria-label="Acter" hidden>
        <li role="none"><span role="menuitem" id="menu-exit" tabindex="-1">Exit</span></li>
      </ul>
    </li>
    <li role="none">
      <span role="menuitem" id="menu-about" aria-haspopup="true" aria-expanded="false" tabindex="-1">About</span>
      <ul role="menu" aria-label="About" hidden>
        <li role="none"><span role="menuitem" id="menu-about-acter" tabindex="-1">About Acter</span></li>
      </ul>
    </li>
  </ul>
  <input id="command-input" />
`;

let actions: { exited: number; abouts: number };
let returned: number;

function byId(id: string): HTMLElement {
  const element = document.getElementById(id);
  if (element === null) {
    throw new Error(`missing element: ${id}`);
  }
  return element;
}

function press(key: string, modifiers: { alt?: boolean } = {}): void {
  const target = document.activeElement ?? document.body;
  target.dispatchEvent(
    new KeyboardEvent('keydown', {
      key,
      altKey: modifiers.alt ?? false,
      bubbles: true,
      cancelable: true,
    }),
  );
}

function release(key: string): void {
  const target = document.activeElement ?? document.body;
  target.dispatchEvent(
    new KeyboardEvent('keyup', { key, bubbles: true, cancelable: true }),
  );
}

function focused(): string {
  return document.activeElement?.id ?? '';
}

function expanded(id: string): string | null {
  return byId(id).getAttribute('aria-expanded');
}

beforeEach(() => {
  document.body.innerHTML = SKELETON;
  actions = { exited: 0, abouts: 0 };
  returned = 0;
  const editField = byId('command-input');
  installMenuBar(
    byId('menu-bar'),
    {
      exit: () => {
        actions.exited += 1;
      },
      about: () => {
        actions.abouts += 1;
      },
    },
    {
      focus: () => {
        returned += 1;
        editField.focus();
      },
    },
  );
  editField.focus();
});

describe('getting in and out', () => {
  it('f10 from anywhere in the window opens the bar on its first item', () => {
    press('F10');

    expect(focused()).toBe('menu-acter');
  });

  /** The key that made a native menu bar worth wanting, and the reason it is answered on
   * keyup: at keydown time Alt+F4 and Alt+Tab are indistinguishable from Alt alone. */
  it('alt pressed and released alone opens the bar', () => {
    press('Alt', { alt: true });
    release('Alt');

    expect(focused()).toBe('menu-acter');
  });

  /** The half that keeps Alt+Tab and every other Alt combination working: anything at all
   * between the press and the release disarms it. */
  it('alt with another key in between does not open anything', () => {
    press('Alt', { alt: true });
    press('Tab', { alt: true });
    release('Alt');

    expect(focused()).toBe('command-input');
  });

  it('f10 again leaves the bar and returns focus to the edit field', () => {
    press('F10');
    press('F10');

    expect(focused()).toBe('command-input');
    expect(returned).toBe(1);
  });

  /** From the bar itself there is nowhere further back, so Escape leaves. What opened it
   * was a key rather than a control, which is why the destination is stated rather than
   * remembered. */
  it('escape on the bar returns focus to the edit field', () => {
    press('F10');
    press('Escape');

    expect(focused()).toBe('command-input');
    expect(returned).toBe(1);
  });
});

describe('walking it', () => {
  it('right and left move between the top level items and wrap', () => {
    press('F10');
    press('ArrowRight');
    expect(focused()).toBe('menu-about');

    press('ArrowRight');
    expect(focused()).toBe('menu-acter');

    press('ArrowLeft');
    expect(focused()).toBe('menu-about');
  });

  it('down opens the menu and lands on its first item', () => {
    press('F10');
    press('ArrowDown');

    expect(focused()).toBe('menu-exit');
    expect(expanded('menu-acter')).toBe('true');
  });

  /** What every menu bar on this platform does: once one menu is open, walking the bar
   * keeps the next one open too. */
  it('walking the bar with a menu open keeps the next one open', () => {
    press('F10');
    press('ArrowDown');
    press('ArrowRight');

    expect(expanded('menu-acter')).toBe('false');
    expect(expanded('menu-about')).toBe('true');
  });

  /** One step back rather than out, which is what a menu user expects. */
  it('escape inside a menu closes it and leaves you on the item that opened it', () => {
    press('F10');
    press('ArrowDown');
    press('Escape');

    expect(focused()).toBe('menu-acter');
    expect(expanded('menu-acter')).toBe('false');
    expect(returned).toBe(0);
  });

  it('up from the bar opens the menu at its last item', () => {
    press('F10');
    press('ArrowRight');
    press('ArrowUp');

    expect(focused()).toBe('menu-about-acter');
  });
});

describe('choosing something', () => {
  it('enter on a leaf runs its action, closes the menu and returns focus', () => {
    press('F10');
    press('ArrowDown');
    press('Enter');

    expect(actions.exited).toBe(1);
    expect(actions.abouts).toBe(0);
    expect(expanded('menu-acter')).toBe('false');
    expect(focused()).toBe('command-input');
  });

  it('the other leaf runs the other action', () => {
    press('F10');
    press('ArrowRight');
    press('ArrowDown');
    press('Enter');

    expect(actions.abouts).toBe(1);
    expect(actions.exited).toBe(0);
  });

  /** Space activates what Enter activates: both are the platform's own, and a menu that
   * answered only one of them would surprise somebody halfway through using it. */
  it('space activates a leaf the way enter does', () => {
    press('F10');
    press('ArrowDown');
    press(' ');

    expect(actions.exited).toBe(1);
  });

  /** The assertion that catches a real bug: every item that can be chosen runs something.
   * An item nobody wired reads as a working menu and does nothing at all. */
  it('every leaf in the bar runs an action', () => {
    const leaves = Array.from(
      document.querySelectorAll<HTMLElement>('[role="menu"] [role="menuitem"]'),
    );

    for (const leaf of leaves) {
      actions = { exited: 0, abouts: 0 };
      leaf.dispatchEvent(new MouseEvent('click', { bubbles: true }));

      expect(
        actions.exited + actions.abouts,
        `${leaf.id} is a menu item nothing happens for`,
      ).toBe(1);
    }

    expect(leaves.length).toBe(2);
  });
});
