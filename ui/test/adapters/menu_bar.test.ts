// @vitest-environment jsdom
// Role: test — the menu bar's keyboard contract.

import { beforeEach, describe, expect, it } from 'vitest';

import { installMenuBar } from '../../src/adapters/menu_bar';

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

let actions: {
  connects: number;
  newConnections: number;
  saves: number;
  exited: number;
  helps: number;
  abouts: number;
};
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
  actions = { connects: 0, newConnections: 0, saves: 0, exited: 0, helps: 0, abouts: 0 };
  returned = 0;
  const editField = byId('command-input');
  installMenuBar(
    byId('menu-bar'),
    {
      connect: () => {
        actions.connects += 1;
      },
      newConnection: () => {
        actions.newConnections += 1;
      },
      saveConnection: () => {
        actions.saves += 1;
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

  it('alt pressed and released alone opens the bar', () => {
    press('Alt', { alt: true });
    release('Alt');

    expect(focused()).toBe('menu-acter');
  });

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

  it('walking the bar with a menu open keeps the next one open', () => {
    press('F10');
    press('ArrowDown');
    press('ArrowRight');

    expect(expanded('menu-acter')).toBe('false');
    expect(expanded('menu-help')).toBe('true');
  });

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

  it('exit is the second item, below connect', () => {
    press('F10');
    press('ArrowDown');
    press('ArrowDown');
    press('Enter');

    expect(actions.exited).toBe(1);
    expect(actions.connects).toBe(0);
  });

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

  // One activation per test: until the adapter's tick moves focus off the item, a second
  // `F10` leaves the bar rather than entering it.
  it('the third menu runs about', () => {
    press('F10');
    press('ArrowRight');
    press('ArrowRight');
    press('ArrowDown');
    press('Enter');

    expect(actions.abouts).toBe(1);
    expect(actions.connects + actions.exited + actions.helps).toBe(0);
  });

  it('space activates a leaf the way enter does', () => {
    press('F10');
    press('ArrowDown');
    press(' ');

    expect(actions.connects).toBe(1);
  });

  it('every leaf in the bar runs an action', () => {
    const leaves = Array.from(
      document.querySelectorAll<HTMLElement>('[role="menu"] [role="menuitem"]'),
    );

    for (const leaf of leaves) {
      actions = { connects: 0, newConnections: 0, saves: 0, exited: 0, helps: 0, abouts: 0 };
      leaf.dispatchEvent(new MouseEvent('click', { bubbles: true }));

      expect(
        actions.connects + actions.exited + actions.helps + actions.abouts,
        `${leaf.id} is a menu item nothing happens for`,
      ).toBe(1);
    }

    expect(leaves.length).toBe(4);
  });
});
