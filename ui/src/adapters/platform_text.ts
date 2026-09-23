// Role: adapter (DOM) — removes text and regions that belong to another operating system.
// Removed rather than hidden: a hidden element is still in the document for anything that walks it.
// `data-platform` lists platforms by `std::env::consts::OS` name, space-separated.

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
