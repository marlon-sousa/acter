// Role: port (driving) — the operating system's own menu bar, as the window hears it.

import type { MenuAction } from '../protocol';

export interface SystemMenuEvents {
  onChosen(chosen: (action: MenuAction) => void): void;
}
