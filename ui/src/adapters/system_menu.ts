// Role: adapter — the operating system's menu bar, wired to the things its items act on.
//
// **The same actions the document menu bar is given** (adapters/menu_bar.ts). Connect from a
// Mac's File menu and Connect from Windows' F10 menu are one path through the application,
// so a change to what connecting means cannot reach one platform and miss the other.
//
// It knows about menus and nothing about sessions, windows or dialogs — the same rule the
// document bar is written under, and the reason both take their actions rather than reaching
// for them.

import type { MenuActions, MenuReturn } from './menu_bar';
import type { SystemMenuEvents } from '../ports/system_menu';

export function installSystemMenu(
  events: SystemMenuEvents,
  actions: MenuActions,
  returnTo: MenuReturn,
): void {
  events.onChosen((action) => {
    // Exhaustive over the protocol's own type: an action added in the backend with no
    // dialog behind it fails to compile here rather than reaching a listener as a menu item
    // that does nothing.
    switch (action) {
      case 'Connect':
        actions.connect();
        break;
      case 'Help':
        actions.help();
        break;
      case 'About':
        actions.about();
        break;
    }
    // **Where focus is once the menu has closed**, and the same fallback the document bar
    // keeps for the same reason: choosing an item takes focus out of the window — on macOS
    // it was in the menu bar, which is not in the window at all — and every action here
    // opens a dialog that claims it back. If one ever does not, a listener must not be left
    // with nothing under them.
    //
    // Checked on the next tick because a dialog's `showModal` places focus in the same turn
    // as the action, and this must read where focus *landed* rather than where it was.
    setTimeout(() => {
      const landed = document.activeElement;
      if (landed === null || landed === document.body) {
        returnTo.focus();
      }
    }, 0);
  });
}
