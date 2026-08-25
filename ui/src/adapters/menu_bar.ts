// Role: adapter (DOM) — the in-document menu bar: the WAI-ARIA menubar keyboard
// contract over the static structure in views/main_window.html.
//
// **Why the menu bar is not native.** Spec A7 measured a Win32 menu bar first, and every
// press of Alt froze NVDA for twenty to sixty seconds — reproduced in a vanilla Tauri
// application built from Tauri's own tutorial, so the cause is the menu's modal loop
// blocking the accessibility calls a reader makes into the webview, not anything Acter
// draws. Focus never leaves the webview here, so that mechanism has nothing to act on.
//
// **F10 is the way in**, which is the platform's own answer to "give me the menu bar"
// and is not swallowed by the webview the way Alt is claimed by the window frame.

/// What the two leaf items do. Passed in rather than reached for, so this adapter knows
/// about menus and nothing about sessions, windows or dialogs.
export interface MenuActions {
  exit(): void;
  about(): void;
}

/// Where focus goes when the menu bar is left: the edit field, always, because what
/// opened the menu was a key rather than a control that can be returned to.
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

  /// The bar keeps exactly one tab stop, on whichever top-level item was last visited:
  /// the roving tabindex the pattern asks for, and what stops Tab from walking every
  /// item in every menu.
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
    // Walking the bar with a menu already open keeps it open, which is how every menu
    // bar on this platform behaves.
    if (wasOpen) {
      open(item);
    }
  }

  /// Which top-level item owns this element, whether it is the item itself or something
  /// inside its submenu.
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
      // The document listens for Escape and F6 of its own (adapters/keyboard.ts). A key
      // this bar has answered is not also theirs.
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
        // From inside a menu, Escape closes it and leaves you on the item that opened
        // it — one step back rather than out, which is what a menu user expects. From
        // the bar itself there is nowhere further back, so it leaves.
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
        // Pressed inside the bar it means "leave", so the same key both enters and
        // leaves rather than trapping the user in a bar they cannot get out of.
        handled();
        leave();
        break;
      default:
        break;
    }
  });

  function activate(item: HTMLElement): void {
    closeAll();
    if (item.id === 'menu-exit') {
      actions.exit();
    } else if (item.id === 'menu-about-acter') {
      actions.about();
    }
    // **Focus goes to the edit field only if the action did not take it somewhere.**
    // Moving it first and letting the action move it again put focus in the edit field
    // for one frame on its way into the dialog, and a reader heard that frame: NVDA
    // announced an unnamed object between the item being chosen and the dialog naming
    // itself (measured 2026-08-24). What is left after a menu closes must never be a
    // hidden menu item, though, which is why the fallback stays.
    setTimeout(() => {
      const landed = document.activeElement;
      if (landed === null || landed === document.body || bar.contains(landed)) {
        returnTo.focus();
      }
    }, 0);
  }

  // Clicking is not the target user's road in, but a menu that cannot be clicked is a
  // menu that behaves unlike every other one on the machine.
  bar.addEventListener('click', (event) => {
    const target = event.target as HTMLElement;
    if (!target.matches(ITEM)) {
      return;
    }
    if (target === ownerOf(target)) {
      // A top-level item: open it and step into it.
      open(target);
      itemsOf(target)[0]?.focus();
    } else {
      activate(target);
    }
  });

  // Focus leaving the bar altogether closes what is open. Checked on the next tick
  // because focusout fires before the new element has focus.
  bar.addEventListener('focusout', () => {
    setTimeout(() => {
      if (!bar.contains(document.activeElement)) {
        closeAll();
      }
    }, 0);
  });

  // Two ways in, both window-level, so they listen on the document like Acter's other
  // window-level keys.
  //
  // **F10** is the platform's own "give me the menu bar", and it is unambiguous: one
  // keystroke, nothing to disambiguate.
  //
  // **Alt on its own** is what a Windows user's hands already do, and it is the reason
  // the native menu bar was worth wanting in the first place. It cannot be answered on
  // keydown, because at that moment Alt+F4, Alt+Tab and every other Alt combination look
  // identical to it — so it is answered on *keyup*, and only if nothing happened in
  // between. `alone` is armed by an Alt keydown with no other modifier and disarmed by
  // anything at all: another key, a click, or the window losing focus, which is how
  // Alt+Tab leaves without opening a menu behind itself.
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
