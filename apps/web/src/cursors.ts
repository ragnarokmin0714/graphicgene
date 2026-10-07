/**
 * Custom cursors, as inline SVG data URLs. Each glyph is drawn twice — a wide
 * white stroke under a thin black one — so it reads on any artwork.
 */

function svgCursor(paths: string, hotspot: [number, number], fallback: string): string {
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke-linecap="round" stroke-linejoin="round">` +
    `<g stroke="white" stroke-width="4">${paths}</g>` +
    `<g stroke="black" stroke-width="1.6">${paths}</g>` +
    `</svg>`;
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}") ${hotspot[0]} ${hotspot[1]}, ${fallback}`;
}

/** A curved double arrow, for the rotate zones outside a frame's corners. */
export const ROTATE_CURSOR = svgCursor(
  `<path d="M6 15a7 7 0 0 1 12 0"/><path d="M4 11l2 4 4-1"/><path d="M20 11l-2 4-4-1"/>`,
  [12, 12],
  "crosshair",
);

/** An eyedropper with its tip on the hotspot. Glyph from lucide's pipette (ISC). */
export const EYEDROPPER_CURSOR = svgCursor(
  `<path d="m12 9-8.414 8.414A2 2 0 0 0 3 18.828v1.344a2 2 0 0 1-.586 1.414A2 2 0 0 1 3.828 21h1.344a2 2 0 0 0 1.414-.586L15 12"/>` +
    `<path d="m18 9 .4.4a1 1 0 1 1-3 3l-3.8-3.8a1 1 0 1 1 3-3l.4.4 3.4-3.4a1 1 0 1 1 3 3z"/>` +
    `<path d="m2 22 .414-.414"/>`,
  [2, 22],
  "crosshair",
);

/** A pen nib with its tip on the hotspot. Glyph from lucide's pen-tool (ISC). */
export const PEN_CURSOR = svgCursor(
  `<path d="M15.707 21.293a1 1 0 0 1-1.414 0l-1.586-1.586a1 1 0 0 1 0-1.414l5.586-5.586a1 1 0 0 1 1.414 0l1.586 1.586a1 1 0 0 1 0 1.414z"/>` +
    `<path d="m18 13-1.375-6.874a1 1 0 0 0-.746-.776L3.235 2.028a1 1 0 0 0-1.207 1.207L5.35 15.879a1 1 0 0 0 .776.746L13 18"/>` +
    `<path d="m2.3 2.3 7.286 7.286"/><circle cx="11" cy="11" r="2"/>`,
  [2, 2],
  "crosshair",
);
