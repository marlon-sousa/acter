// @vitest-environment jsdom
// Role: test — EditFieldDom over a real input, and above all its selection question.

import { beforeEach, describe, expect, it } from 'vitest';

import { EditFieldDom } from '../../src/adapters/edit_field';

let input: HTMLInputElement;
let field: EditFieldDom;

beforeEach(() => {
  document.body.innerHTML = '';
  input = document.createElement('input');
  input.type = 'text';
  document.body.append(input);
  field = new EditFieldDom(input);
});

describe('EditFieldDom', () => {
  it('reads, clears and focuses the input', () => {
    input.value = '  small  ';
    expect(field.value()).toBe('  small  ');

    field.focus();
    expect(field.isFocused()).toBe(true);

    field.clear();
    expect(input.value).toBe('');
  });
});

describe('EditFieldDom.hasSelection', () => {
  it('is false with only a caret', () => {
    input.value = 'git status';
    input.focus();
    input.setSelectionRange(4, 4);

    expect(field.hasSelection()).toBe(false);
  });

  it('is true over a selected range', () => {
    input.value = 'git status';
    input.focus();
    input.setSelectionRange(0, 3);

    expect(field.hasSelection()).toBe(true);
  });

  it('sees a range the document Selection API cannot', () => {
    input.value = 'git status';
    input.focus();
    input.setSelectionRange(0, 3);

    expect(window.getSelection()?.toString() ?? '').toBe('');
    expect(field.hasSelection()).toBe(true);
  });
});
