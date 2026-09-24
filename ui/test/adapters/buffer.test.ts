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
    buffer.applyLine(2, 10, 'Appended', 'file-a');
    buffer.applyLine(1, 11, 'Appended', 'on branch main');
    buffer.applyLine(2, 10, 'Appended', 'file-b');

    const headings = region.querySelectorAll('h2');
    expect(Array.from(headings).map((h) => h.textContent)).toEqual([
      'git status',
      'ls',
    ]);
    const gitOutput = headings[0]?.nextElementSibling;
    const lsOutput = headings[1]?.nextElementSibling;
    expect(gitOutput?.textContent).toBe('on branch main');
    expect(lsOutput?.textContent).toBe('file-afile-b');
  });

  it('updates the heading when reopened with a real line, ignoring empty reopens', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, '');
    buffer.applyLine(1, 4, 'Appended', 'early chunk');
    buffer.openBlock(1, 'git status');
    buffer.openBlock(1, '');

    const headings = region.querySelectorAll('h2');
    expect(headings).toHaveLength(1);
    expect(headings[0]?.textContent).toBe('git status');
    expect(headings[0]?.nextElementSibling?.textContent).toBe('early chunk');
  });

  it('gives a block with no command line no heading at all, and still shows its text', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'echo hello');
    buffer.applyLine(1, 5, 'Appended', 'hello');
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

    it('leaves a three-item list three lines after three presses', () => {
      const region = makeRegion();
      const buffer = new BufferDom(region);
      buffer.openBlock(1, 'gh pr create');
      const items = ['acter', 'upstream', 'Skip pushing the branch'];
      items.forEach((item, at) => {
        buffer.applyLine(1, at, 'Appended', `  ${item}`);
      });
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

  it('goes away again when it is cleared', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'git status');

    buffer.clear();

    expect(region.hidden).toBe(true);
  });
});

describe('a heading that repeats the line above it', () => {
  function echoed(region: HTMLElement): boolean[] {
    return Array.from(region.querySelectorAll('h2')).map((h) => h.classList.contains('echoed'));
  }

  it('is marked when the row above ends with its text', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, "Read-Host 'Your name'");
    buffer.applyLine(1, 1, 'Appended', 'Your name: Marlon');
    buffer.openBlock(2, 'Marlon');

    expect(echoed(region)).toEqual([false, true]);
  });

  it('is not marked when the row above says something else', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'echo hello');
    buffer.applyLine(1, 1, 'Appended', 'hello');
    buffer.openBlock(2, 'git status');

    expect(echoed(region)).toEqual([false, false]);
  });

  it('is not marked when the row above is only the same text', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'echo ls');
    buffer.applyLine(1, 1, 'Appended', 'ls');
    buffer.openBlock(2, 'ls');

    expect(echoed(region)).toEqual([false, false]);
  });

  it('is not marked when the match starts inside a word', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'cat names');
    buffer.applyLine(1, 1, 'Appended', 'dials');
    buffer.openBlock(2, 'ls');

    expect(echoed(region)).toEqual([false, false]);
  });

  it('is not marked after a prompt paragraph', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.appendPrompt('PS C:\>');
    buffer.openBlock(1, 'Get-Date');

    expect(echoed(region)).toEqual([false]);
  });

  it('follows the row above when it arrives or changes after the heading', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, '');
    buffer.openBlock(2, "read -p 'Name: ' n");
    expect(echoed(region)).toEqual([false]);

    buffer.applyLine(1, 1, 'Appended', "marlon@splyt:~$ read -p 'Name: ' n");
    expect(echoed(region)).toEqual([true]);

    buffer.applyLine(1, 1, 'Rewritten', 'something else');
    expect(echoed(region)).toEqual([false]);
  });

  it('follows its own text when the block is renamed', () => {
    const region = makeRegion();
    const buffer = new BufferDom(region);
    buffer.openBlock(1, 'x');
    buffer.applyLine(1, 1, 'Appended', 'Your name: Marlon');
    buffer.openBlock(2, 'typo');
    expect(echoed(region)).toEqual([false, false]);

    buffer.openBlock(2, 'Marlon');
    expect(echoed(region)).toEqual([false, true]);
  });
});
