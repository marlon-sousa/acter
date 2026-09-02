// @vitest-environment jsdom
// Role: test — what the window does when somebody chooses an item in the operating system's
// own menu bar.
//
// The menu itself cannot be reached from here: it is built natively, outside the webview,
// and its own assertions are in acter-core's policy and the PR's VoiceOver checklist (spec
// M3, decision 7). What *is* here is everything after the choice arrives — which action
// runs, and where focus is left — because that is the half a listener experiences.

import { beforeEach, describe, expect, it, vi } from 'vitest';

import { installSystemMenu } from '../../src/adapters/system_menu';
import type { MenuAction } from '../../src/protocol';

/** The backend's side of the port, with the emission under the test's control. */
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

  /// Quit, hide, copy and the rest are the platform's and are answered by it. Nothing in
  /// this menu ends the application through Acter's own exit, which is what the Windows
  /// menu's Exit item does and what a Mac's Cmd+Q does for itself (spec M3, decision 3).
  it('never reaches the exit action, because quitting is the platform own item', () => {
    const menu = menuEvents();
    const ran = actions();
    installSystemMenu(menu.events, ran, { focus: () => undefined });

    for (const action of ['Connect', 'Help', 'About'] as const) {
      menu.choose(action);
    }
    expect(ran.exit).not.toHaveBeenCalled();
  });

  /// The failure this protects against is a listener standing on nothing: choosing an item
  /// took focus out of the window — on macOS it was in the menu bar, which is not in the
  /// window at all — so an action that opens nothing must hand it back.
  it('puts focus back in the window when the action placed none', async () => {
    const menu = menuEvents();
    const returned = vi.fn();
    installSystemMenu(menu.events, actions(), { focus: returned });

    menu.choose('Connect');
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(returned).toHaveBeenCalledOnce();
  });

  /// And the other half of the same rule, which is why the check is on the next tick rather
  /// than immediately: an action that opened a dialog has already placed focus, and taking
  /// it back would pull a listener out of the dialog they were just given.
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
