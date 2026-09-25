/**
 * The boundary to the Rust core.
 *
 * The document lives in Rust. This module is the *only* place that touches the
 * wasm instance, and nothing above it may keep a copy of document state —
 * React renders a view and sends commands back. The moment React holds
 * authoritative state, the web and desktop apps start behaving differently and
 * the core stops being the product.
 *
 * It is also the batching boundary: one call per user interaction, never one
 * call per node.
 */
import init, { Editor } from "./wasm/graphicgene_wasm.js";

export type LayerRow = {
  id: string;
  /** 0 for top-level layers. Rows arrive in panel order: topmost first. */
  depth: number;
  name: string;
  kind: "group" | "vector";
  visible: boolean;
  locked: boolean;
  opacity: number;
  selected: boolean;
};

export type Rgba = [number, number, number, number];

/** A document-space point. At 100% zoom, also a canvas pixel. */
export type Point = readonly [number, number];

/** The selection frame: corners go top-left, top-right, bottom-right, bottom-left, in frame terms. */
export type Frame = {
  corners: readonly [Point, Point, Point, Point];
  width: number;
  height: number;
};

/** A line from an anchor to one of its handles: ax, ay, hx, hy. */
export type HandleLine = readonly [number, number, number, number];

/** The pen's in-progress path. */
export type PenOverlay = {
  anchors: Point[];
  /** Handles of the anchor being placed. */
  handles: HandleLine[];
  /** SVG path data from the last anchor to the pointer. */
  preview: string | null;
  /** A press now would close the path on its first anchor. */
  closable: boolean;
};

/** The path whose anchors are being edited. */
export type PathOverlay = {
  outline: string;
  anchors: { at: Point; selected: boolean }[];
  /** Handles of the selected anchors. */
  handles: HandleLine[];
};

export type EditorMode = "pen" | "path";

/** What the selection overlay draws, all in document space. */
export type Overlay = {
  mode: EditorMode | null;
  frame: Frame | null;
  /** SVG path data for each selected node. */
  outlines: string[];
  /** SVG path data for the node under the pointer, unless it is selected. */
  hover: string | null;
  /** x0, y0, x1, y1 */
  marquee: readonly [number, number, number, number] | null;
  gesture: "move" | "scale" | "rotate" | "create" | "marquee" | null;
  pen?: PenOverlay;
  path?: PathOverlay;
};

export type ShapeKind = "rect" | "ellipse";

/** What a select-tool press should turn into; see `selectAt` in the wasm crate. */
export type PressOutcome = "drag" | "hit" | "miss";

/** What a press while editing a path landed on. */
export type PathPressOutcome = "handle" | "anchor" | "segment" | "miss";

let ready: Promise<void> | null = null;

/** Load the wasm module once, however many components ask for it. */
export function loadCore(): Promise<void> {
  ready ??= init().then(() => undefined);
  return ready;
}

export class EditorHandle {
  private constructor(private readonly inner: Editor) {}

  static async create(width: number, height: number): Promise<EditorHandle> {
    await loadCore();
    return new EditorHandle(new Editor(width, height));
  }

  resize(width: number, height: number): void {
    this.inner.resize(width, height);
  }

  addRect(x: number, y: number, w: number, h: number, color: Rgba): string {
    return this.inner.addRect(x, y, w, h, new Uint8Array(color));
  }

  addEllipse(cx: number, cy: number, rx: number, ry: number, color: Rgba): string {
    return this.inner.addEllipse(cx, cy, rx, ry, new Uint8Array(color));
  }

  /** The pen tool sends a whole SVG path, not one call per segment. */
  addPath(svgPath: string, color: Rgba): string {
    return this.inner.addPath(svgPath, new Uint8Array(color));
  }

  setTransform(id: string, m: readonly number[]): void {
    this.inner.setTransform(id, new Float64Array(m));
  }

  undo(): boolean {
    return this.inner.undo();
  }

  redo(): boolean {
    return this.inner.redo();
  }

  get canUndo(): boolean {
    return this.inner.canUndo();
  }

  get canRedo(): boolean {
    return this.inner.canRedo();
  }

  // Selection. Lives in the core because it drives transforms; see lib.rs.

  selectAt(x: number, y: number, additive: boolean): PressOutcome {
    return this.inner.selectAt(x, y, additive) as PressOutcome;
  }

  selectLayer(id: string, additive: boolean): void {
    this.inner.selectLayer(id, additive);
  }

  selectAll(): void {
    this.inner.selectAll();
  }

  clearSelection(): void {
    this.inner.clearSelection();
  }

  get selectionCount(): number {
    return this.inner.selectionCount();
  }

  /** Track the node under the pointer; true if that changed. */
  hover(x: number, y: number): boolean {
    return this.inner.hover(x, y);
  }

  clearHover(): boolean {
    return this.inner.clearHover();
  }

  deleteSelection(): boolean {
    return this.inner.deleteSelection();
  }

  nudge(dx: number, dy: number): boolean {
    return this.inner.nudge(dx, dy);
  }

  // Gestures: begin* on press, updateGesture per move, endGesture on release.
  // The whole drag lands in the journal as one undo step.

  beginMove(x: number, y: number): boolean {
    return this.inner.beginMove(x, y);
  }

  /** (u, v) picks the handle in unit frame coordinates: corners are 0|1, edge midpoints 0.5. */
  beginScale(u: number, v: number, x: number, y: number): boolean {
    return this.inner.beginScale(u, v, x, y);
  }

  beginRotate(x: number, y: number): boolean {
    return this.inner.beginRotate(x, y);
  }

  beginCreate(shape: ShapeKind, x: number, y: number, color: Rgba): void {
    this.inner.beginCreate(shape, x, y, new Uint8Array(color));
  }

  beginMarquee(x: number, y: number, additive: boolean): void {
    this.inner.beginMarquee(x, y, additive);
  }

  updateGesture(x: number, y: number, shift: boolean, alt: boolean): void {
    this.inner.updateGesture(x, y, shift, alt);
  }

  /** Returns the new node's id when the gesture drew a shape. */
  endGesture(): string | undefined {
    return this.inner.endGesture();
  }

  cancelGesture(): boolean {
    return this.inner.cancelGesture();
  }

  // Pen and path editing. `tolerance` is a pick distance in document units:
  // a screen distance divided by the zoom.

  /** The first press starts a path; later ones add anchors. */
  penPress(x: number, y: number, shift: boolean, tolerance: number, color: Rgba): void {
    this.inner.penPress(x, y, shift, tolerance, new Uint8Array(color));
  }

  penDrag(x: number, y: number, shift: boolean): void {
    this.inner.penDrag(x, y, shift);
  }

  /** Returns the path's id when the press completed it (closed or ended). */
  penRelease(): string | undefined {
    return this.inner.penRelease();
  }

  /** Track the pointer between presses; true while a path is being drawn. */
  penHover(x: number, y: number, tolerance: number): boolean {
    return this.inner.penHover(x, y, tolerance);
  }

  /** Finish the pen path or stop editing a path; true if either was active. */
  finishMode(): boolean {
    return this.inner.finishMode();
  }

  get mode(): EditorMode | null {
    return (this.inner.mode() as EditorMode | undefined) ?? null;
  }

  /** Edit the anchors of the one selected path; false if that is not possible. */
  beginPathEdit(): boolean {
    return this.inner.beginPathEdit();
  }

  pathPress(x: number, y: number, tolerance: number, additive: boolean): PathPressOutcome {
    return this.inner.pathPress(x, y, tolerance, additive) as PathPressOutcome;
  }

  pathDrag(x: number, y: number, shift: boolean, alt: boolean): void {
    this.inner.pathDrag(x, y, shift, alt);
  }

  pathRelease(): boolean {
    return this.inner.pathRelease();
  }

  pathCancelDrag(): void {
    this.inner.pathCancelDrag();
  }

  /** Toggles corner/smooth on an anchor; returns whether editing continues. */
  pathDoubleClick(x: number, y: number, tolerance: number): boolean {
    return this.inner.pathDoubleClick(x, y, tolerance);
  }

  overlay(): Overlay {
    return JSON.parse(this.inner.overlay()) as Overlay;
  }

  /** RGBA bytes for the whole canvas. */
  render(): Uint8Array {
    return this.inner.render();
  }

  layers(): LayerRow[] {
    return JSON.parse(this.inner.layerTree()) as LayerRow[];
  }

  /**
   * Core does no IO — it hands back bytes and the caller decides where they
   * go. IndexedDB here; `std::fs` in the future desktop app.
   */
  toJson(): string {
    return this.inner.toJson();
  }

  loadJson(text: string): void {
    this.inner.loadJson(text);
  }
}
