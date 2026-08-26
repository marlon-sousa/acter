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

  // B4.9's manual pass, on the prompt a bare Enter brings back: it arrives with nothing
  // open, so it is a block for text no command accounts for. An empty h2 there is a
  // level 2 heading that announces nothing and reads as a dead end — DESIGN says such
  // text gets a block with *no heading*, and this is that rendered honestly.
  it('gives a block with no command line no heading at all, and still shows its text', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'echo hello');
    buffer.appendOutput(1, 'hello');
    // The shell's own text: a returning prompt nobody submitted.
    buffer.openBlock(2, '');
    buffer.appendOutput(2, 'C:\\Users\\marlo>');

    const headings = region.querySelectorAll('h2');
    expect(Array.from(headings).map((h) => h.textContent)).toEqual([
      'echo hello',
    ]);
    expect(region.textContent).toContain('C:\\Users\\marlo>');
  });

  it('creates the heading in front of the output when a block is named later', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, '');
    buffer.appendOutput(1, 'early chunk');
    buffer.openBlock(1, 'git status');

    const heading = region.querySelector('h2');
    expect(heading?.textContent).toBe('git status');
    // Reads in the order it would have had all along: heading, then its output.
    expect(heading?.nextElementSibling?.textContent).toBe('early chunk');
    expect(heading?.getAttribute('tabindex')).toBe('-1');
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

// The buffer is emptied between one session ending and the next being attached to
// (spec B7, decision 1), so no shell's output is ever appended under another's heading.
describe('clear', () => {
  it('empties the region so nothing of the previous session is left to navigate', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'git status');
    buffer.appendOutput(1, 'on branch main');
    buffer.appendPrompt('C:\>');

    buffer.clear();

    expect(region.querySelectorAll('h2')).toHaveLength(0);
    expect(region.textContent).toBe('');
  });

  /** The map of blocks goes with the DOM. The next session's ids start again at 1, so a
   * remembered block would take the new shell's first command and append its output under
   * the old shell's heading — a transcript of a session that never happened. */
  it('forgets its blocks, so the next command opens a new one', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'git status');

    buffer.clear();
    buffer.openBlock(1, 'ls');

    const headings = region.querySelectorAll('h2');
    expect(headings).toHaveLength(1);
    expect(headings[0]?.textContent).toBe('ls');
  });
});

// **The buffer is in the document only while it has something in it** (spec A10). Since B7
// an empty buffer is what every launch opens with rather than a state that lasts until the
// first prompt arrives, and a region a listener arrows onto to hear nothing is worse than no
// region at all.
describe('being there at all', () => {
  it('is hidden until something is put in it', () => {
    const region = makeRegion();
    region.hidden = true;
    const buffer = new BufferDom(region);

    expect(region.hidden).toBe(true);

    buffer.openBlock(1, 'git status');
    expect(region.hidden).toBe(false);
  });

  it('appears for output and for a prompt as well as for a block', () => {
    for (const put of [
      (buffer: BufferDom) => {
        buffer.appendPrompt('C:\>');
      },
      (buffer: BufferDom) => {
        buffer.openBlock(1, 'ls');
        buffer.appendOutput(1, 'a file');
      },
    ]) {
      const region = makeRegion();
      region.hidden = true;
      const buffer = new BufferDom(region);

      put(buffer);

      expect(region.hidden).toBe(false);
    }
  });

  /** Clearing is what happens between one session and the next, so the empty buffer of a
   * window that has just connected is not in the document either. */
  it('goes away again when it is cleared', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'git status');

    buffer.clear();

    expect(region.hidden).toBe(true);
  });
});
