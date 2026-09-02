// Role: adapter (DOM) — text and regions that belong to one operating system, removed on
// the others.
//
// **Removed rather than hidden**, which is the rule A7 wrote for the menu-bar region and
// this generalises: a hidden element is still in the document for anything that walks it,
// and what a listener must never meet is a region that is there and says nothing. So an
// element marked for a platform this is not simply stops existing, before the window says
// anything.
//
// **One attribute, listing the platforms an element belongs to.** `data-platform="windows"`
// keeps it on Windows only; `data-platform="macos linux"` keeps it on either. The names are
// the ones the backend answers with (`std::env::consts::OS`), so there is one vocabulary for
// "which platform is this" across the whole application rather than a second one invented
// here.
//
// It exists because two things are now true at once (spec M3, decision 8): the menu bar is in
// the document on Windows and in the system bar on macOS, so the sentence in the help that
// says how to reach it cannot be one sentence.

export function applyPlatformText(root: ParentNode, os: string): void {
  for (const element of Array.from(
    root.querySelectorAll<HTMLElement>('[data-platform]'),
  )) {
    const belongs = (element.dataset.platform ?? '').split(/\s+/).filter(Boolean);
    if (!belongs.includes(os)) {
      element.remove();
    }
  }
}
