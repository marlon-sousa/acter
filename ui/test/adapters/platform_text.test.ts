// @vitest-environment jsdom
// Role: test — what a listener meets when part of the window belongs to another operating
// system.

import { beforeEach, describe, expect, it } from 'vitest';

import { applyPlatformText } from '../../src/adapters/platform_text';

const MARKUP = `
  <div id="menu-bar-region" data-platform="windows">the menu bar in the document</div>
  <p id="windows-keys" data-platform="windows">F10 opens the menu bar.</p>
  <p id="macos-keys" data-platform="macos">The menus are at the top of the screen.</p>
  <p id="either" data-platform="macos linux">A shell on this computer.</p>
  <p id="everywhere">F1 opens this help.</p>
`;

describe('text that belongs to one platform', () => {
  beforeEach(() => {
    document.body.innerHTML = MARKUP;
  });

  it('keeps what this platform claims and removes the rest', () => {
    applyPlatformText(document, 'windows');

    expect(document.getElementById('windows-keys')).not.toBeNull();
    expect(document.getElementById('menu-bar-region')).not.toBeNull();
    expect(document.getElementById('macos-keys')).toBeNull();
    expect(document.getElementById('either')).toBeNull();
  });

  it('keeps an element that names more than one platform, on each of them', () => {
    applyPlatformText(document, 'macos');

    expect(document.getElementById('either')).not.toBeNull();
    expect(document.getElementById('macos-keys')).not.toBeNull();
    expect(document.getElementById('windows-keys')).toBeNull();
  });

  it('leaves anything unmarked alone', () => {
    applyPlatformText(document, 'linux');

    expect(document.getElementById('everywhere')).not.toBeNull();
    expect(document.getElementById('either')).not.toBeNull();
  });

  it('removes rather than hides, so nothing empty is left to meet', () => {
    applyPlatformText(document, 'macos');

    expect(document.querySelector('[data-platform="windows"]')).toBeNull();
    expect(document.body.textContent).not.toContain('F10 opens the menu bar');
  });

  it('shows a platform nothing was written for only what is common', () => {
    applyPlatformText(document, 'freebsd');

    expect(document.getElementById('everywhere')).not.toBeNull();
    expect(document.querySelectorAll('[data-platform]')).toHaveLength(0);
  });
});
