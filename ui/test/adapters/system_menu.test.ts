// @vitest-environment jsdom
// Role: test — what the window does when somebody chooses an item in the operating system's
// own menu bar.

import { beforeEach, describe, expect, it, vi } from 'vitest';

import { installSystemMenu } from '../../src/adapters/system_menu';
import type { MenuAction } from '../../src/protocol';

function menuEvents(): {
  choose: (action: MenuAction) => void;
  events: { onChosen(chosen: (action: MenuAction) => void): void };
} {
  let listener: (action: MenuAction) => void = () => undefined;
  return {
    choose: (action) => listener(action),
    events: {
      onChosen(chosen) {
        listener = chosen;
      },
    },
  };
}

function actions() {
  return {
    connect: vi.fn(),
    newConnection: vi.fn(),
    saveConnection: vi.fn(),
    exit: vi.fn(),
    help: vi.fn(),
    about: vi.fn(),
  };
}

describe('the operating system menu', () => {
  beforeEach(() => {
    document.body.innerHTML = '<input id="command-input" />';
  });

  it('runs the action the chosen item names', () => {
    const menu = menuEvents();
    const ran = actions();
    installSystemMenu(menu.events, ran, { focus: () => undefined });

    menu.choose('Connect');
    expect(ran.connect).toHaveBeenCalledOnce();

    menu.choose('Help');
    expect(ran.help).toHaveBeenCalledOnce();

    menu.choose('About');
    expect(ran.about).toHaveBeenCalledOnce();
  });

  it('never reaches the exit action, because quitting is the platform own item', () => {
    const menu = menuEvents();
    const ran = actions();
    installSystemMenu(menu.events, ran, { focus: () => undefined });

    for (const action of ['Connect', 'Help', 'About'] as const) {
      menu.choose(action);
    }
    expect(ran.exit).not.toHaveBeenCalled();
  });

  it('puts focus back in the window when the action placed none', async () => {
    const menu = menuEvents();
    const returned = vi.fn();
    installSystemMenu(menu.events, actions(), { focus: returned });

    menu.choose('Connect');
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(returned).toHaveBeenCalledOnce();
  });

  it('leaves focus alone when the action placed it somewhere', async () => {
    const menu = menuEvents();
    const returned = vi.fn();
    const ran = actions();
    ran.connect.mockImplementation(() => {
      document.getElementById('command-input')?.focus();
    });
    installSystemMenu(menu.events, ran, { focus: returned });

    menu.choose('Connect');
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(returned).not.toHaveBeenCalled();
    expect(document.activeElement?.id).toBe('command-input');
  });
});
