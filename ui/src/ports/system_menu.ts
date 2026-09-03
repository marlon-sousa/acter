// Role: port (driving) — the operating system's own menu bar, as the window hears it.
//
// **Only one direction, and deliberately.** The frontend does not build this menu, does not
// name it and cannot change it: the backend decides what the platform's menu holds, because
// that is a platform decision and the platform is a fact the compiler already knows (spec
// M3, decision 1). What reaches the window is the one thing only the window can answer —
// somebody chose an item that opens a dialog.
//
// Windows has no implementation of this and needs none: its menu bar is in the document and
// its items call the same actions directly (spec A7).

import type { MenuAction } from '../protocol';

export interface SystemMenuEvents {
  /**
   * Call `chosen` every time a menu item Acter owns is picked.
   *
   * Subscribing is not a question with an answer, so nothing is returned: the window lives
   * as long as the menu does, and a listener that could be torn down would be a lifetime
   * nobody has a use for.
   */
  onChosen(chosen: (action: MenuAction) => void): void;
}
