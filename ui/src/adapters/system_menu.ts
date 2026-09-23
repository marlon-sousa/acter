// Role: adapter — the operating system's menu bar, wired to the things its items act on.

import type { MenuActions, MenuReturn } from './menu_bar';
import type { SystemMenuEvents } from '../ports/system_menu';

export function installSystemMenu(
  events: SystemMenuEvents,
  actions: MenuActions,
  returnTo: MenuReturn,
): void {
  events.onChosen((action) => {
    switch (action) {
      case 'Connect':
        actions.connect();
        break;
      case 'NewConnection':
        actions.newConnection();
        break;
      case 'SaveConnection':
        actions.saveConnection();
        break;
      case 'Help':
        actions.help();
        break;
      case 'About':
        actions.about();
        break;
    }
    // Next tick, because `showModal` places focus in the same turn as the action.
    setTimeout(() => {
      const landed = document.activeElement;
      if (landed === null || landed === document.body) {
        returnTo.focus();
      }
    }, 0);
  });
}
