/**
 * Selection handles: where they sit, which one the pointer is on, and which
 * cursor that deserves.
 *
 * The frame's corners come from the core. Handle size and grab distance are
 * screen measurements, which is why this part lives in the UI rather than in
 * the core: at 200% zoom a handle is still 8 screen pixels.
 */
import type { Frame, Point } from "@/editor";

/** Drawn handle size, in px. */
export const HANDLE_SIZE = 8;
/** How close to a handle's centre counts as grabbing it. */
const GRAB_RADIUS = 6;
/** How far outside a corner the rotate zone reaches. */
const ROTATE_RADIUS = 18;
/** Edges shorter than this drop their midpoint handles so corners stay grabbable. */
const MIN_EDGE_FOR_MIDPOINTS = 24;

export type Handle = { u: number; v: number; at: Point };

export type HandleTarget =
  | { kind: "scale"; u: number; v: number; cursor: string }
  | { kind: "rotate"; cursor: string };

/** The frame point at unit coordinates (u, v). Frames are parallelograms. */
export function pointAt(frame: Frame, u: number, v: number): Point {
  const [a, b, , d] = frame.corners;
  return [a[0] + u * (b[0] - a[0]) + v * (d[0] - a[0]), a[1] + u * (b[1] - a[1]) + v * (d[1] - a[1])];
}

export function handlesOf(frame: Frame): Handle[] {
  const units: [number, number][] = [
    [0, 0],
    [1, 0],
    [1, 1],
    [0, 1],
  ];
  if (frame.width >= MIN_EDGE_FOR_MIDPOINTS) units.push([0.5, 0], [0.5, 1]);
  if (frame.height >= MIN_EDGE_FOR_MIDPOINTS) units.push([0, 0.5], [1, 0.5]);
  return units.map(([u, v]) => ({ u, v, at: pointAt(frame, u, v) }));
}

/** Which handle, if any, a press at `p` grabs. Scale handles win over rotation. */
export function handleAt(frame: Frame, p: Point): HandleTarget | null {
  const center = pointAt(frame, 0.5, 0.5);
  for (const handle of handlesOf(frame)) {
    if (distance(handle.at, p) <= GRAB_RADIUS) {
      return { kind: "scale", u: handle.u, v: handle.v, cursor: resizeCursor(center, handle.at) };
    }
  }
  if (!inside(frame, p) && frame.corners.some((corner) => distance(corner, p) <= ROTATE_RADIUS)) {
    return { kind: "rotate", cursor: ROTATE_CURSOR };
  }
  return null;
}

/**
 * The resize cursor pointing from the frame centre towards a handle, so a
 * rotated frame still shows arrows along the direction the handle moves.
 */
function resizeCursor(center: Point, handle: Point): string {
  const degrees = (Math.atan2(handle[1] - center[1], handle[0] - center[0]) * 180) / Math.PI;
  const folded = ((degrees % 180) + 180) % 180;
  if (folded < 22.5 || folded >= 157.5) return "ew-resize";
  if (folded < 67.5) return "nwse-resize";
  if (folded < 112.5) return "ns-resize";
  return "nesw-resize";
}

function inside(frame: Frame, p: Point): boolean {
  const [a, b, , d] = frame.corners;
  const e1 = [b[0] - a[0], b[1] - a[1]];
  const e2 = [d[0] - a[0], d[1] - a[1]];
  const det = e1[0] * e2[1] - e1[1] * e2[0];
  if (Math.abs(det) < 1e-9) return false;
  const q = [p[0] - a[0], p[1] - a[1]];
  const u = (q[0] * e2[1] - q[1] * e2[0]) / det;
  const v = (e1[0] * q[1] - e1[1] * q[0]) / det;
  return u >= 0 && u <= 1 && v >= 0 && v <= 1;
}

function distance(a: Point, b: Point): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1]);
}

/** A curved double arrow, outlined in white so it reads on any artwork. */
const ROTATE_CURSOR = `url("data:image/svg+xml,${encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke-linecap="round" stroke-linejoin="round">` +
    `<g stroke="white" stroke-width="4"><path d="M6 15a7 7 0 0 1 12 0"/><path d="M4 11l2 4 4-1"/><path d="M20 11l-2 4-4-1"/></g>` +
    `<g stroke="black" stroke-width="1.6"><path d="M6 15a7 7 0 0 1 12 0"/><path d="M4 11l2 4 4-1"/><path d="M20 11l-2 4-4-1"/></g>` +
    `</svg>`,
)}") 12 12, crosshair`;
