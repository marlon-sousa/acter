// @vitest-environment jsdom
// Role: test — the far end's command line as a real element: what it announces itself as,
// what Acter writes into it, and where the caret lands.

import { beforeEach, describe, expect, it } from 'vitest';

import { FarEndFieldDom } from '../../src/adapters/far_end_field';

function build(): { field: HTMLElement; container: HTMLElement; dom: FarEndFieldDom } {
  document.body.innerHTML = `
    <div id="far-end-line" hidden>
      <label id="far-end-label" for="far-end-input">Command line</label>
      <span
        id="far-end-input"
        contenteditable="true"
        role="textbox"
        aria-multiline="false"
        aria-labelledby="far-end-label"
        tabindex="0"
        spellcheck="false"
      ></span>
    </div>`;
  const field = document.getElementById('far-end-input') as HTMLElement;
  const container = document.getElementById('far-end-line') as HTMLElement;
  return { field, container, dom: new FarEndFieldDom(field, container) };
}

function caretAt(): number {
  const selection = window.getSelection();
  return selection === null ? -1 : selection.getRangeAt(0).startOffset;
}

let built = build();

beforeEach(() => {
  built = build();
});

describe('what the element announces itself as', () => {
  it('is an editable single-line text box with a name', () => {
    const { field } = built;

    expect(field.getAttribute('role')).toBe('textbox');
    expect(field.getAttribute('contenteditable')).toBe('true');
    expect(field.getAttribute('aria-multiline')).toBe('false');
    expect(
      document.getElementById(
        field.getAttribute('aria-labelledby') ?? '',
      )?.textContent,
    ).toBe('Command line');
  });

  it('is not an application', () => {
    expect(built.field.getAttribute('role')).not.toBe('application');
  });
});

describe('what Acter writes into it', () => {
  it('holds the row the far end drew', () => {
    built.dom.render('cargo test --all', 16);

    expect(built.field.textContent).toBe('cargo test --all');
    expect(caretAt()).toBe(16);
  });

  it('moves the caret without touching the text when no row changed', () => {
    built.dom.render('cargo test --all', 16);

    built.dom.render(null, 3);

    expect(built.field.textContent).toBe('cargo test --all');
    expect(caretAt()).toBe(3);
  });

  it('empties the row when the far end emptied it', () => {
    built.dom.render('some command', 12);

    built.dom.render('', 0);

    expect(built.field.textContent).toBe('');
    expect(caretAt()).toBe(0);
  });

  it('clamps a caret past the end of the row to its end', () => {
    built.dom.render('ls', 99);

    expect(caretAt()).toBe(2);
  });

  it('places the caret in an empty field without throwing', () => {
    expect(() => built.dom.render('', 4)).not.toThrow();
    expect(built.field.textContent).toBe('');
  });
});

describe('being there at all', () => {
  it('is out of the document until the far end owns the line', () => {
    expect(built.container.hidden).toBe(true);

    built.dom.show(true);
    expect(built.container.hidden).toBe(false);

    built.dom.show(false);
    expect(built.container.hidden).toBe(true);
  });

  it('takes focus and says when it has it', () => {
    built.dom.show(true);

    built.dom.focus();

    expect(built.dom.isFocused()).toBe(true);
    expect(document.activeElement).toBe(built.field);
  });

  describe('a completion', () => {
    it('leaves what it added selected, so the reader says it', () => {
      const { field, dom } = build();
      dom.render('ech', 3);

      dom.render('echo ', 5, true);

      const selection = window.getSelection();
      expect(selection?.toString()).toBe('o ');
      expect(field.textContent).toBe('echo ');
    });

    it("drops the selection again, because the row is the far end's and not a suggestion", async () => {
      const { dom } = build();
      dom.render('ech', 3);
      dom.render('echo ', 5, true);

      await new Promise((resolve) => setTimeout(resolve, 200));

      const selection = window.getSelection();
      expect(selection?.toString()).toBe('');
      expect(selection?.isCollapsed).toBe(true);
    });

    it('says nothing extra when the row was rewritten rather than added to', () => {
      const { dom } = build();
      dom.render('echo one', 8);

      dom.render('echo two', 8, true);

      expect(window.getSelection()?.isCollapsed).toBe(true);
    });

    it('is not applied to an ordinary answer', () => {
      const { dom } = build();
      dom.render('ech', 3);

      dom.render('echo ', 5);

      expect(window.getSelection()?.isCollapsed).toBe(true);
    });
  });
});
