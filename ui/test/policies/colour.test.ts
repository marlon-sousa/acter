// Role: test — the colour policy: Campbell, the xterm table and the 4.5:1 floor.

import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

import {
  CAMPBELL,
  MINIMUM_CONTRAST,
  SESSION_BG,
  SESSION_FG,
  contrast,
  drawn,
  hex,
  parse,
  readable,
  xterm256,
} from '../../src/policies/colour';
import type { Colour, Style } from '../../src/protocol';

const background = parse(SESSION_BG);

function style(overrides: Partial<Style> = {}): Style {
  return {
    fg: null,
    bg: null,
    bold: false,
    dim: false,
    italic: false,
    underline: false,
    inverse: false,
    strike: false,
    ...overrides,
  };
}

function named(index: number): Colour {
  return { kind: 'Named', index };
}

function hue(value: string): number {
  const { r, g, b } = parse(value);
  return Math.atan2(Math.sqrt(3) * (g - b), 2 * r - g - b);
}

describe('Campbell against the session background', () => {
  const below = [1, 4, 5, 8, 13];

  it('has below 4.5:1 exactly the colours the spec measured', () => {
    const measured = CAMPBELL.map((c, index) => ({ index, ratio: contrast(parse(c), background) }))
      .filter(({ index }) => index !== 0)
      .filter(({ ratio }) => ratio < MINIMUM_CONTRAST)
      .map(({ index }) => index);
    expect(measured).toEqual(below);
  });

  it('raises each colour below the floor to 4.5:1, only just, and keeps its hue', () => {
    for (const index of below) {
      const original = CAMPBELL[index] ?? '';
      const color = drawn(style({ fg: named(index) })).color ?? '';
      const ratio = contrast(parse(color), background);
      expect(ratio, `colour ${index}`).toBeGreaterThanOrEqual(MINIMUM_CONTRAST);
      expect(ratio, `colour ${index}`).toBeLessThan(4.7);
      if (index !== 8) {
        expect(Math.abs(hue(color) - hue(original)), `colour ${index}`).toBeLessThan(0.05);
      }
    }
  });

  it('leaves every colour already at or above the floor as it is', () => {
    for (const index of [2, 3, 6, 7, 9, 10, 11, 12, 14, 15]) {
      const color = drawn(style({ fg: named(index) })).color;
      expect(color ?? SESSION_FG, `colour ${index}`).toBe(CAMPBELL[index]);
    }
  });
});

describe('the 256-colour table', () => {
  it('matches xterm at its corners, cube steps and grey ramp', () => {
    const xterm: Record<number, string> = {
      16: '#000000',
      17: '#00005f',
      21: '#0000ff',
      22: '#005f00',
      46: '#00ff00',
      52: '#5f0000',
      67: '#5f87af',
      88: '#870000',
      124: '#af0000',
      160: '#d70000',
      196: '#ff0000',
      208: '#ff8700',
      226: '#ffff00',
      231: '#ffffff',
      232: '#080808',
      244: '#808080',
      250: '#bcbcbc',
      255: '#eeeeee',
    };
    for (const [index, value] of Object.entries(xterm)) {
      expect(hex(xterm256(Number(index))), `index ${index}`).toBe(value);
    }
  });

  it('gives an index below 16 its Campbell colour', () => {
    expect(hex(xterm256(1))).toBe(CAMPBELL[1]);
  });
});

describe('drawing a style', () => {
  it('draws the session colours as no colour at all', () => {
    expect(drawn(style())).toEqual({
      color: null,
      background: null,
      bold: false,
      italic: false,
      underline: false,
      strike: false,
    });
  });

  it('uses an Rgb colour as given when it is readable', () => {
    expect(drawn(style({ fg: { kind: 'Rgb', r: 255, g: 128, b: 0 } })).color).toBe('#ff8000');
  });

  it('swaps the session colours for inverse', () => {
    const inverse = drawn(style({ inverse: true }));
    expect(inverse.background).toBe(SESSION_FG);
    expect(inverse.color).toBe(SESSION_BG);
  });

  it('keeps a foreground readable against the run background, darkening it on a light one', () => {
    const light = drawn(style({ bg: named(15) }));
    expect(light.background).toBe(CAMPBELL[15]);
    const ratio = contrast(parse(light.color ?? ''), parse(CAMPBELL[15] ?? ''));
    expect(ratio).toBeGreaterThanOrEqual(MINIMUM_CONTRAST);
  });

  it('draws dim darker, and never below the floor', () => {
    const dim = drawn(style({ dim: true })).color ?? '';
    expect(parse(dim).r).toBeLessThan(parse(SESSION_FG).r);
    const dimRed = drawn(style({ fg: named(1), dim: true })).color ?? '';
    expect(contrast(parse(dimRed), background)).toBeGreaterThanOrEqual(MINIMUM_CONTRAST);
  });

  it('carries bold, italic, underline and strike through', () => {
    const all = drawn(style({ bold: true, italic: true, underline: true, strike: true }));
    expect([all.bold, all.italic, all.underline, all.strike]).toEqual([true, true, true, true]);
  });

  it('agrees with the stylesheet about the session colours', () => {
    const css = readFileSync(new URL('../../src/styles.css', import.meta.url), 'utf8');
    expect(css).toContain(`--session-bg: ${SESSION_BG};`);
    expect(css).toContain(`--session-fg: ${SESSION_FG};`);
  });

  it('keeps any colour at the floor', () => {
    expect(contrast(readable(parse('#000080'), background), background)).toBeGreaterThanOrEqual(
      MINIMUM_CONTRAST,
    );
  });
});
