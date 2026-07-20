// @vitest-environment jsdom
// Role: test — BufferDom block-keying and focus-landing contract in a real DOM.

import { describe, expect, it } from 'vitest';

import { BufferDom } from '../../src/adapters/buffer';

function makeRegion(): HTMLElement {
  const region = document.createElement('div');
  region.setAttribute('role', 'region');
  region.setAttribute('aria-label', 'Results');
  region.tabIndex = -1;
  document.body.append(region);
  return region;
}

describe('BufferDom blocks', () => {
  it('opens an h2 block per command and appends chunks under the matching one', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'git status');
    buffer.openBlock(2, 'ls');
    // Chunks arrive out of block order but land under their own command's block.
    buffer.appendOutput(2, 'file-a');
    buffer.appendOutput(1, 'on branch main');
    buffer.appendOutput(2, 'file-b');

    const headings = region.querySelectorAll('h2');
    expect(Array.from(headings).map((h) => h.textContent)).toEqual([
      'git status',
      'ls',
    ]);
    // The output region for each block is the heading's next sibling.
    const gitOutput = headings[0]?.nextElementSibling;
    const lsOutput = headings[1]?.nextElementSibling;
    expect(gitOutput?.textContent).toBe('on branch main');
    expect(lsOutput?.textContent).toBe('file-afile-b');
  });

  it('updates the heading when reopened with a real line, ignoring empty reopens', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    // An event opened the block first with an empty heading (ack not yet arrived).
    buffer.openBlock(1, '');
    buffer.appendOutput(1, 'early chunk');
    // The ack arrives and sets the authoritative command line.
    buffer.openBlock(1, 'git status');
    // A later empty reopen (e.g. a duplicate) must not clobber the line.
    buffer.openBlock(1, '');

    const headings = region.querySelectorAll('h2');
    expect(headings).toHaveLength(1);
    expect(headings[0]?.textContent).toBe('git status');
    // The early chunk is preserved under the same block.
    expect(headings[0]?.nextElementSibling?.textContent).toBe('early chunk');
  });

  it('ignores output for a command with no open block rather than throwing', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    expect(() => buffer.appendOutput(99, 'orphan')).not.toThrow();
    expect(region.querySelectorAll('h2')).toHaveLength(0);
  });
});

describe('BufferDom.focus', () => {
  it('lands on the most recent command heading', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'git status');
    buffer.openBlock(2, 'ls');

    buffer.focus();

    const active = document.activeElement as HTMLElement;
    expect(active.tagName).toBe('H2');
    expect(active.textContent).toBe('ls');
  });

  it('falls back to the region container when the buffer is empty', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);

    buffer.focus();

    expect(document.activeElement).toBe(region);
  });

  it('gives every appended heading tabindex="-1"', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'git status');
    buffer.openBlock(2, 'ls');

    const headings = region.querySelectorAll('h2');
    expect(headings).toHaveLength(2);
    for (const heading of headings) {
      expect(heading.getAttribute('tabindex')).toBe('-1');
    }
  });
});
