// Role: policy — what a style run looks like on screen: Campbell, the xterm table, and the 4.5:1 floor.

import type { Colour, Style } from '../protocol';

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

export const CAMPBELL: readonly string[] = [
  '#0c0c0c',
  '#c50f1f',
  '#13a10e',
  '#c19c00',
  '#0037da',
  '#881798',
  '#3a96dd',
  '#cccccc',
  '#767676',
  '#e74856',
  '#16c60c',
  '#f9f1a5',
  '#3b78ff',
  '#b4009e',
  '#61d6d6',
  '#f2f2f2',
];

export const SESSION_FG = '#cccccc';
export const SESSION_BG = '#0c0c0c';

export const MINIMUM_CONTRAST = 4.5;

// Alacritty's own dim factor.
const DIM = 0.66;

const CUBE = [0, 95, 135, 175, 215, 255];

export function xterm256(index: number): Rgb {
  if (index < 16) {
    return parse(CAMPBELL[index] ?? SESSION_FG);
  }
  if (index < 232) {
    const at = index - 16;
    return {
      r: CUBE[Math.floor(at / 36)] ?? 0,
      g: CUBE[Math.floor(at / 6) % 6] ?? 0,
      b: CUBE[at % 6] ?? 0,
    };
  }
  const grey = 8 + (index - 232) * 10;
  return { r: grey, g: grey, b: grey };
}

export function parse(hex: string): Rgb {
  const value = Number.parseInt(hex.slice(1), 16);
  return { r: (value >> 16) & 0xff, g: (value >> 8) & 0xff, b: value & 0xff };
}

export function hex({ r, g, b }: Rgb): string {
  return `#${[r, g, b].map((c) => Math.round(c).toString(16).padStart(2, '0')).join('')}`;
}

// WCAG 2 relative luminance and contrast ratio.
function luminance({ r, g, b }: Rgb): number {
  const linear = (c: number): number => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
}

export function contrast(a: Rgb, b: Rgb): number {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((light ?? 0) + 0.05) / ((dark ?? 0) + 0.05);
}

interface Hsl {
  h: number;
  s: number;
  l: number;
}

function toHsl({ r, g, b }: Rgb): Hsl {
  const [rr, gg, bb] = [r / 255, g / 255, b / 255];
  const max = Math.max(rr, gg, bb);
  const min = Math.min(rr, gg, bb);
  const l = (max + min) / 2;
  if (max === min) {
    return { h: 0, s: 0, l };
  }
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h =
    max === rr ? (gg - bb) / d + (gg < bb ? 6 : 0) : max === gg ? (bb - rr) / d + 2 : (rr - gg) / d + 4;
  return { h: h / 6, s, l };
}

function fromHsl({ h, s, l }: Hsl): Rgb {
  if (s === 0) {
    return { r: l * 255, g: l * 255, b: l * 255 };
  }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const channel = (t: number): number => {
    const u = t < 0 ? t + 1 : t > 1 ? t - 1 : t;
    if (u < 1 / 6) return p + (q - p) * 6 * u;
    if (u < 1 / 2) return q;
    if (u < 2 / 3) return p + (q - p) * (2 / 3 - u) * 6;
    return p;
  };
  return { r: channel(h + 1 / 3) * 255, g: channel(h) * 255, b: channel(h - 1 / 3) * 255 };
}

// Moves the lightness away from the background, keeping hue and saturation, to the least change that reaches the floor.
export function readable(fg: Rgb, bg: Rgb): Rgb {
  if (contrast(fg, bg) >= MINIMUM_CONTRAST) {
    return fg;
  }
  const hsl = toHsl(fg);
  const white = { r: 255, g: 255, b: 255 };
  const black = { r: 0, g: 0, b: 0 };
  const lighter = contrast(white, bg) >= contrast(black, bg);
  let near = hsl.l;
  let far = lighter ? 1 : 0;
  for (let step = 0; step < 24; step++) {
    const mid = (near + far) / 2;
    const rounded = round(fromHsl({ ...hsl, l: mid }));
    if (contrast(rounded, bg) >= MINIMUM_CONTRAST) {
      far = mid;
    } else {
      near = mid;
    }
  }
  return round(fromHsl({ ...hsl, l: far }));
}

function round({ r, g, b }: Rgb): Rgb {
  return { r: Math.round(r), g: Math.round(g), b: Math.round(b) };
}

function resolve(colour: Colour | null, fallback: string): Rgb {
  if (colour === null) {
    return parse(fallback);
  }
  switch (colour.kind) {
    case 'Named':
    case 'Indexed':
      return xterm256(colour.index);
    case 'Rgb':
      return { r: colour.r, g: colour.g, b: colour.b };
  }
}

export interface Drawn {
  // Null where the session's own colour already shows.
  color: string | null;
  background: string | null;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strike: boolean;
}

export function drawn(style: Style): Drawn {
  let fg = resolve(style.fg, SESSION_FG);
  let bg = resolve(style.bg, SESSION_BG);
  if (style.inverse) {
    [fg, bg] = [bg, fg];
  }
  if (style.dim) {
    fg = { r: fg.r * DIM, g: fg.g * DIM, b: fg.b * DIM };
  }
  const color = hex(readable(round(fg), bg));
  const background = hex(bg);
  return {
    color: color === SESSION_FG ? null : color,
    background: background === SESSION_BG ? null : background,
    bold: style.bold,
    italic: style.italic,
    underline: style.underline,
    strike: style.strike,
  };
}
