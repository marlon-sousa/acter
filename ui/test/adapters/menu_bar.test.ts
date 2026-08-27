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
        <li role="none"><span role="menuitem" id="menu-connect" tabindex="-1">Connect</span></li>
        <li role="none"><span role="menuitem" id="menu-exit" tabindex="-1">Exit</span></li>
      </ul>
    </li>
    <li role="none">
      <span role="menuitem" id="menu-help" aria-haspopup="true" aria-expanded="false" tabindex="-1">Help</span>
      <ul role="menu" aria-label="Help" hidden>
        <li role="none"><span role="menuitem" id="menu-acter-help" tabindex="-1">Acter help</span></li>
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

let actions: { connects: number; exited: number; helps: number; abouts: number };
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
  actions = { connects: 0, exited: 0, helps: 0, abouts: 0 };
  returned = 0;
  const editField = byId('command-input');
  installMenuBar(
    byId('menu-bar'),
    {
      connect: () => {
        actions.connects += 1;
      },
      exit: () => {
        actions.exited += 1;
      },
      help: () => {
        actions.helps += 1;
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
    expect(focused()).toBe('menu-help');

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

    expect(focused()).toBe('menu-connect');
    expect(expanded('menu-acter')).toBe('true');
  });

  /** What every menu bar on this platform does: once one menu is open, walking the bar
   * keeps the next one open too. */
  it('walking the bar with a menu open keeps the next one open', () => {
    press('F10');
    press('ArrowDown');
    press('ArrowRight');

    expect(expanded('menu-acter')).toBe('false');
    expect(expanded('menu-help')).toBe('true');
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
    press('ArrowRight');
    press('ArrowUp');

    expect(focused()).toBe('menu-about-acter');
  });
});

describe('choosing something', () => {
  it('enter on a leaf runs its action and closes the menu', () => {
    press('F10');
    press('ArrowDown');
    press('Enter');

    expect(actions.connects).toBe(1);
    expect(actions.exited + actions.helps + actions.abouts).toBe(0);
    expect(expanded('menu-acter')).toBe('false');
  });

  /** Connect is the item this menu is mostly opened for since B7, and Exit is below it —
   * so the one that ends the application is not what an accidental Enter lands on. */
  it('exit is the second item, below connect', () => {
    press('F10');
    press('ArrowDown');
    press('ArrowDown');
    press('Enter');

    expect(actions.exited).toBe(1);
    expect(actions.connects).toBe(0);
  });

  /** Focus must not be left on a hidden menu item, so it falls back to the edit field —
   * but only after the action has had its chance to take focus somewhere of its own.
   * Moving it eagerly put focus in the edit field for one frame on the way into the About
   * dialog, and NVDA announced that frame as an unnamed object. */
  it('focus falls back to the edit field when the action did not take it', async () => {
    press('F10');
    press('ArrowDown');
    press('Enter');

    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(focused()).toBe('command-input');
    expect(returned).toBe(1);
  });

  it('an action that takes focus itself keeps it', async () => {
    const elsewhere = document.createElement('button');
    elsewhere.id = 'elsewhere';
    document.body.append(elsewhere);
    press('F10');
    press('ArrowRight');
    press('ArrowDown');
    // The About dialog does exactly this: it opens and takes focus.
    elsewhere.focus();
    press('Enter');

    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(returned).toBe(0);
  });

  it('the second menu runs help', () => {
    press('F10');
    press('ArrowRight');
    press('ArrowDown');
    press('Enter');

    expect(actions.helps).toBe(1);
    expect(actions.connects + actions.exited + actions.abouts).toBe(0);
  });

  // One activation per test, deliberately: activating leaves focus on the item until the
  // adapter's own tick moves it, so a second `F10` in the same test means *leave the bar*
  // rather than enter it — which is the bar behaving correctly and a test walking into it.
  it('the third menu runs about', () => {
    press('F10');
    press('ArrowRight');
    press('ArrowRight');
    press('ArrowDown');
    press('Enter');

    expect(actions.abouts).toBe(1);
    expect(actions.connects + actions.exited + actions.helps).toBe(0);
  });

  /** Space activates what Enter activates: both are the platform's own, and a menu that
   * answered only one of them would surprise somebody halfway through using it. */
  it('space activates a leaf the way enter does', () => {
    press('F10');
    press('ArrowDown');
    press(' ');

    expect(actions.connects).toBe(1);
  });

  /** The assertion that catches a real bug: every item that can be chosen runs something.
   * An item nobody wired reads as a working menu and does nothing at all. */
  it('every leaf in the bar runs an action', () => {
    const leaves = Array.from(
      document.querySelectorAll<HTMLElement>('[role="menu"] [role="menuitem"]'),
    );

    for (const leaf of leaves) {
      actions = { connects: 0, exited: 0, helps: 0, abouts: 0 };
      leaf.dispatchEvent(new MouseEvent('click', { bubbles: true }));

      expect(
        actions.connects + actions.exited + actions.helps + actions.abouts,
        `${leaf.id} is a menu item nothing happens for`,
      ).toBe(1);
    }

    expect(leaves.length).toBe(4);
  });
});
