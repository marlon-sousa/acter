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
    buffer.applyLine(2, 10, 'Appended', 'file-a');
    buffer.applyLine(1, 11, 'Appended', 'on branch main');
    buffer.applyLine(2, 10, 'Appended', 'file-b');

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
    buffer.applyLine(1, 4, 'Appended', 'early chunk');
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
    buffer.applyLine(1, 5, 'Appended', 'hello');
    // The shell's own text: a returning prompt nobody submitted.
    buffer.openBlock(2, '');
    buffer.applyLine(2, 6, 'Appended', 'C:\\Users\\marlo>');

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
    buffer.applyLine(1, 7, 'Appended', 'early chunk');
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
    expect(() => buffer.applyLine(99, 8, 'Appended', 'orphan')).not.toThrow();
    expect(region.querySelectorAll('h2')).toHaveLength(0);
  });
});

  // **The buffer applies revisions by id since 28** (decision 8), and this is what it buys
  // at a far end: the transcript is the far end's own record rather than a line per press.
  //
  // Measured against `gh pr create` on 2026-09-02: each arrow rewrites exactly two rows, and
  // answering the prompt blanks the option rows and rewrites the question row to carry the
  // answer. A buffer that could only append would show every intermediate state of a list
  // the far end had already erased.
  describe('applying revisions by id', () => {
    it('rewrites a line in place rather than appending another', () => {
      const region = makeRegion();
      const buffer = new BufferDom(region);
      buffer.openBlock(1, 'ssh');
      buffer.applyLine(1, 7, 'Appended', '$ echo one');
      buffer.applyLine(1, 7, 'Rewritten', '$ echo two');
      buffer.applyLine(1, 7, 'Rewritten', '$ exit');

      const output = region.querySelector('.response');
      expect(output?.children).toHaveLength(1);
      expect(output?.textContent).toBe('$ exit');
    });

    /// The definition-of-done case, stated as a listener would meet it: three presses down a
    /// three-item list leave three lines, not nine.
    it('leaves a three-item list three lines after three presses', () => {
      const region = makeRegion();
      const buffer = new BufferDom(region);
      buffer.openBlock(1, 'gh pr create');
      const items = ['acter', 'upstream', 'Skip pushing the branch'];
      items.forEach((item, at) => {
        buffer.applyLine(1, at, 'Appended', `  ${item}`);
      });
      // Three presses, each moving the marker one row down.
      for (let selected = 0; selected < 3; selected++) {
        items.forEach((item, at) => {
          const marker = at === selected ? '>' : ' ';
          buffer.applyLine(1, at, 'Rewritten', `${marker} ${item}`);
        });
      }

      const output = region.querySelector('.response');
      expect(output?.children).toHaveLength(3);
      expect(Array.from(output?.children ?? []).map((row) => row.textContent)).toEqual([
        '  acter',
        '  upstream',
        '> Skip pushing the branch',
      ]);
    });

    /// A row the far end erased reads as erased rather than disappearing: the vertical
    /// structure is what a listener navigates by, and a line that vanished from under them
    /// mid-read is worse than a blank one.
    it('renders a blank as a blank rather than removing the line', () => {
      const region = makeRegion();
      const buffer = new BufferDom(region);
      buffer.openBlock(1, 'gh pr create');
      buffer.applyLine(1, 1, 'Appended', '  an option');
      buffer.applyLine(1, 1, 'Rewritten', '');

      const output = region.querySelector('.response');
      expect(output?.children).toHaveLength(1);
      expect(output?.textContent).toBe('');
    });

    /// A settlement carries the row whole, exactly as a rewrite does — it is the line's
    /// final word rather than another piece of it.
    it('treats a settlement as the line whole', () => {
      const region = makeRegion();
      const buffer = new BufferDom(region);
      buffer.openBlock(1, 'ls');
      buffer.applyLine(1, 3, 'Appended', 'partial');
      buffer.applyLine(1, 3, 'Settled', 'partial and complete');

      expect(region.querySelector('.response')?.textContent).toBe(
        'partial and complete',
      );
    });

    /// Two lines are two elements, so a listener arrowing the buffer meets them one at a
    /// time — which is the whole reason the buffer is a document rather than a string.
    it('gives each line an element of its own', () => {
      const region = makeRegion();
      const buffer = new BufferDom(region);
      buffer.openBlock(1, 'ls');
      buffer.applyLine(1, 1, 'Appended', 'one.txt');
      buffer.applyLine(1, 2, 'Appended', 'two.txt');

      const output = region.querySelector('.response');
      expect(Array.from(output?.children ?? []).map((row) => row.textContent)).toEqual([
        'one.txt',
        'two.txt',
      ]);
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
    buffer.applyLine(1, 9, 'Appended', 'on branch main');
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
        buffer.applyLine(1, 10, 'Appended', 'a file');
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
