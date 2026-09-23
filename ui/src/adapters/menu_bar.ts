// Role: adapter (DOM) — the in-document menu bar: the WAI-ARIA menubar keyboard
// contract over the static structure in views/main_window.html.

export interface MenuActions {
  connect(): void;
  newConnection(): void;
  saveConnection(): void;
  exit(): void;
  help(): void;
  about(): void;
}

export interface MenuReturn {
  focus(): void;
}

const ITEM = '[role="menuitem"]';

export function installMenuBar(
  bar: HTMLElement,
  actions: MenuActions,
  returnTo: MenuReturn,
): void {
  const top = Array.from(bar.querySelectorAll<HTMLElement>(`:scope > li > ${ITEM}`));

  function itemsOf(parent: HTMLElement): HTMLElement[] {
    const submenu = submenuOf(parent);
    return submenu === null
      ? []
      : Array.from(submenu.querySelectorAll<HTMLElement>(ITEM));
  }

  function submenuOf(item: HTMLElement): HTMLElement | null {
    return item.parentElement?.querySelector<HTMLElement>('[role="menu"]') ?? null;
  }

  function moveTabStop(to: HTMLElement): void {
    for (const item of top) {
      item.tabIndex = item === to ? 0 : -1;
    }
  }

  function open(item: HTMLElement): void {
    const submenu = submenuOf(item);
    if (submenu === null) {
      return;
    }
    closeAll();
    submenu.hidden = false;
    item.setAttribute('aria-expanded', 'true');
  }

  function closeAll(): void {
    for (const item of top) {
      const submenu = submenuOf(item);
      if (submenu !== null) {
        submenu.hidden = true;
        item.setAttribute('aria-expanded', 'false');
      }
    }
  }

  function leave(): void {
    closeAll();
    returnTo.focus();
  }

  function focusTop(index: number): void {
    const item = top[(index + top.length) % top.length];
    if (item === undefined) {
      return;
    }
    const wasOpen = bar.querySelector('[aria-expanded="true"]') !== null;
    closeAll();
    moveTabStop(item);
    item.focus();
    if (wasOpen) {
      open(item);
    }
  }

  function ownerOf(element: HTMLElement): HTMLElement | undefined {
    return top.find(
      (item) => item === element || submenuOf(item)?.contains(element) === true,
    );
  }

  bar.addEventListener('keydown', (event) => {
    const target = event.target as HTMLElement;
    const owner = ownerOf(target);
    if (owner === undefined) {
      return;
    }
    const inSubmenu = target !== owner;
    const siblings = itemsOf(owner);
    const handled = () => {
      event.preventDefault();
      // The document's own Escape and F6 listeners must not also answer this key.
      event.stopPropagation();
    };

    switch (event.key) {
      case 'ArrowRight':
        handled();
        focusTop(top.indexOf(owner) + 1);
        break;
      case 'ArrowLeft':
        handled();
        focusTop(top.indexOf(owner) - 1);
        break;
      case 'ArrowDown':
        handled();
        if (inSubmenu) {
          siblings[(siblings.indexOf(target) + 1) % siblings.length]?.focus();
        } else {
          open(owner);
          siblings[0]?.focus();
        }
        break;
      case 'ArrowUp':
        handled();
        if (inSubmenu) {
          siblings[
            (siblings.indexOf(target) - 1 + siblings.length) % siblings.length
          ]?.focus();
        } else {
          open(owner);
          siblings[siblings.length - 1]?.focus();
        }
        break;
      case 'Home':
        handled();
        (inSubmenu ? siblings[0] : top[0])?.focus();
        break;
      case 'End':
        handled();
        (inSubmenu ? siblings[siblings.length - 1] : top[top.length - 1])?.focus();
        break;
      case 'Escape':
        handled();
        if (inSubmenu) {
          closeAll();
          owner.focus();
        } else {
          leave();
        }
        break;
      case 'Enter':
      case ' ':
        handled();
        if (inSubmenu) {
          activate(target);
        } else {
          open(owner);
          siblings[0]?.focus();
        }
        break;
      case 'F10':
        handled();
        leave();
        break;
      default:
        break;
    }
  });

  function activate(item: HTMLElement): void {
    closeAll();
    if (item.id === 'menu-connect') {
      actions.connect();
    } else if (item.id === 'menu-new-connection') {
      actions.newConnection();
    } else if (item.id === 'menu-save-connection') {
      actions.saveConnection();
    } else if (item.id === 'menu-exit') {
      actions.exit();
    } else if (item.id === 'menu-acter-help') {
      actions.help();
    } else if (item.id === 'menu-about-acter') {
      actions.about();
    }
    // Focus must not visit the edit field on its way into a dialog: NVDA announces that frame.
    setTimeout(() => {
      const landed = document.activeElement;
      if (landed === null || landed === document.body || bar.contains(landed)) {
        returnTo.focus();
      }
    }, 0);
  }

  bar.addEventListener('click', (event) => {
    const target = event.target as HTMLElement;
    if (!target.matches(ITEM)) {
      return;
    }
    if (target === ownerOf(target)) {
      open(target);
      itemsOf(target)[0]?.focus();
    } else {
      activate(target);
    }
  });

  // Checked on the next tick because focusout fires before the new element has focus.
  bar.addEventListener('focusout', () => {
    setTimeout(() => {
      if (!bar.contains(document.activeElement)) {
        closeAll();
      }
    }, 0);
  });

  // Alt alone opens the bar on keyup, because on keydown it looks like Alt+F4 or Alt+Tab; any
  // other key, a mousedown or a blur disarms it.
  let alone = false;

  document.addEventListener('keydown', (event) => {
    if (event.key === 'F10') {
      alone = false;
      event.preventDefault();
      if (bar.contains(document.activeElement)) {
        leave();
      } else {
        focusTop(0);
      }
      return;
    }
    alone =
      event.key === 'Alt' && !event.ctrlKey && !event.shiftKey && !event.metaKey;
  });

  document.addEventListener('keyup', (event) => {
    if (event.key !== 'Alt' || !alone) {
      return;
    }
    alone = false;
    event.preventDefault();
    if (bar.contains(document.activeElement)) {
      leave();
    } else {
      focusTop(0);
    }
  });

  for (const disarm of ['mousedown', 'blur'] as const) {
    window.addEventListener(disarm, () => {
      alone = false;
    });
  }
}
