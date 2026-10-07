/**
 * Colour maths for the picker: HSV and hex over sRGB bytes.
 *
 * Display maths only. The document keeps colours as linear floats, and the
 * core converts the sRGB bytes it is sent; nothing here is the colour model,
 * and nothing here converts to or from linear light.
 */
import type { Rgba } from "@/editor";

/** Fills for new shapes, cycled so a fresh canvas is not a wall of one colour; also the picker's presets. */
export const SWATCHES: readonly Rgba[] = [
  [99, 102, 241, 255],
  [244, 114, 94, 255],
  [20, 184, 166, 255],
  [245, 176, 65, 255],
  [217, 70, 239, 255],
];

/** Hue in degrees (0–360), saturation and value 0–1. */
export type Hsv = { h: number; s: number; v: number };

export function isRgba(color: unknown): color is Rgba {
  return Array.isArray(color);
}

export function rgbToHsv([r, g, b]: Rgba | readonly [number, number, number]): Hsv {
  const [rf, gf, bf] = [r / 255, g / 255, b / 255];
  const max = Math.max(rf, gf, bf);
  const delta = max - Math.min(rf, gf, bf);
  let h = 0;
  if (delta > 0) {
    if (max === rf) h = ((gf - bf) / delta) % 6;
    else if (max === gf) h = (bf - rf) / delta + 2;
    else h = (rf - gf) / delta + 4;
    h = (h * 60 + 360) % 360;
  }
  return { h, s: max === 0 ? 0 : delta / max, v: max };
}

export function hsvToRgb({ h, s, v }: Hsv): [number, number, number] {
  const channel = (n: number) => {
    const k = (n + h / 60) % 6;
    return Math.round((v - v * s * Math.max(0, Math.min(k, 4 - k, 1))) * 255);
  };
  return [channel(5), channel(3), channel(1)];
}

/** "6366F1": what the hex field shows. */
export function toHex([r, g, b]: Rgba): string {
  return [r, g, b].map((c) => c.toString(16).padStart(2, "0")).join("").toUpperCase();
}

/** Six hex digits or three, with or without "#"; null if neither. */
export function parseHex(text: string): [number, number, number] | null {
  let digits = text.trim().replace(/^#/, "");
  if (/^[0-9a-f]{3}$/i.test(digits)) digits = digits.replace(/./g, (d) => d + d);
  if (!/^[0-9a-f]{6}$/i.test(digits)) return null;
  const value = parseInt(digits, 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}

/** For a CSS `background`. */
export function toCss([r, g, b, a]: Rgba): string {
  return `rgb(${r} ${g} ${b} / ${a / 255})`;
}
