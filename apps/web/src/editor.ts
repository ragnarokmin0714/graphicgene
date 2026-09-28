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
 *
 * Positions and pick tolerances passed in are *screen* pixels — CSS pixels
 * from the viewport's corner — and the overlay comes back in screen pixels
 * too. The core maps them through the view (zoom and pan); nothing on this
 * side converts coordinates.
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

/** A colour as sRGB bytes with straight alpha, 0–255 each. */
export type Rgba = [number, number, number, number];

/** A value the selected nodes do not share. */
export type Mixed = "mixed";

/**
 * What the properties panel shows. Position and size are in document units,
 * rotation in degrees counter-clockwise, opacity 0–1.
 */
export type Properties = {
  count: number;
  /** The selection frame's top-left corner, in the frame's own orientation. */
  x: number;
  y: number;
  width: number;
  height: number;
  /** Always 0 for several nodes: they share an unrotated box. */
  rotation: number;
  opacity: number | Mixed;
  /** `null` is no fill; left out when only groups are selected. */
  fill?: Rgba | null | Mixed;
  /** The stroke colour, as `fill`: `null` is no stroke. */
  stroke?: Rgba | null | Mixed;
  /** Of the strokes there are; left out when nothing is stroked. */
  strokeWidth?: number | Mixed;
};

/** One change made through the properties panel. */
export type PropertyChange =
  | { x: number }
  | { y: number }
  | { width: number }
  | { height: number }
  | { rotation: number }
  | { opacity: number }
  | { fill: Rgba | null }
  /** Removes strokes; `strokeColor` adds them. */
  | { stroke: null }
  /** Recolours strokes, adding a thin one where there is none. */
  | { strokeColor: Rgba }
  /** Re-widths the strokes there are. */
  | { strokeWidth: number };

/** A point in screen pixels: CSS pixels from the viewport's top-left corner. */
export type Point = readonly [number, number];

/** The selection frame: corners go top-left, top-right, bottom-right, bottom-left, in frame terms. */
export type Frame = {
  corners: readonly [Point, Point, Point, Point];
  /** In document units, for the size label. */
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

/** What the overlay draws, in screen pixels. */
export type Overlay = {
  mode: EditorMode | null;
  frame: Frame | null;
  /** SVG path data for each selected node. */
  outlines: string[];
  /** SVG path data for the node under the pointer, unless it is selected. */
  hover: string | null;
  /** x0, y0, x1, y1 */
  marquee: readonly [number, number, number, number] | null;
  /** Something selected is locked: its frame shows, but it has no handles. */
  locked: boolean;
  gesture: "move" | "scale" | "rotate" | "create" | "marquee" | null;
  pen?: PenOverlay;
  path?: PathOverlay;
  /** Where the artboard is on screen (x0, y0, x1, y1), and its size in document units. */
  artboard: { rect: readonly [number, number, number, number]; width: number; height: number };
};

export type ShapeKind = "rect" | "ellipse";

/** Where dragged layers land, relative to a row: in front of it, behind it, or into it. */
export type DropPlace = "above" | "below" | "inside";

/** A step through the stacking order. */
export type Arrangement = "forward" | "backward" | "front" | "back";

/** What a select-tool press should turn into; see `selectAt` in the wasm crate. */
export type PressOutcome = "drag" | "hit" | "miss";

/** What a press while editing a path landed on. */
export type PathPressOutcome = "handle" | "anchor" | "segment" | "miss";

/** An area of the canvas: x, y, width, height, in device pixels. */
export type PixelRect = readonly [number, number, number, number];

/**
 * How to bring the canvas up to date: shift what it shows by `shift`, then
 * put back each of `rects` from `pixels()`. Nothing to do when both are empty.
 */
export type Repaint = { shift: readonly [number, number]; rects: PixelRect[] };

let ready: Promise<void> | null = null;
/** The wasm module's memory, where the canvas pixels live. */
let memory: WebAssembly.Memory | null = null;

/** Load the wasm module once, however many components ask for it. */
export function loadCore(): Promise<void> {
  ready ??= init().then((exports) => {
    memory = exports.memory;
  });
  return ready;
}

export class EditorHandle {
  private constructor(private readonly inner: Editor) {}

  static async create(width: number, height: number): Promise<EditorHandle> {
    await loadCore();
    return new EditorHandle(new Editor(width, height));
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

  /** `tolerance`: how far outside a shape still hits it, in screen pixels. */
  selectAt(x: number, y: number, additive: boolean, tolerance: number): PressOutcome {
    return this.inner.selectAt(x, y, additive, tolerance) as PressOutcome;
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
  hover(x: number, y: number, tolerance: number): boolean {
    return this.inner.hover(x, y, tolerance);
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

  /**
   * Bring the pixels up to date and say how to repaint the canvas: after a
   * pan, shift it and put back the strips that appeared; after an edit, put
   * back the damaged area. Nothing, after a selection click or a hover.
   */
  render(): Repaint {
    const out = this.inner.render();
    const rects: PixelRect[] = [];
    for (let i = 0; i < out[2]; i++) {
      const at = 3 + i * 4;
      rects.push([out[at], out[at + 1], out[at + 2], out[at + 3]]);
    }
    return { shift: [out[0], out[1]], rects };
  }

  /**
   * Call once a pan has come to rest. Shifted pixels can be a hair off along
   * edges the old canvas edge cut; true means the next `render` redraws
   * everything to make them exact.
   */
  settle(): boolean {
    return this.inner.settle();
  }

  // The view: zoom and pan. Per-viewer, never saved; the core owns the maths.

  /** The viewport in device pixels, and the device pixel ratio. The first call fits the artboard. */
  setViewport(width: number, height: number, dpr: number): void {
    this.inner.setViewport(width, height, dpr);
  }

  /** Screen pixels per document unit: 1 is 100%. */
  get zoom(): number {
    return this.inner.zoom;
  }

  /** Where the document origin is on screen, in CSS pixels. */
  get pan(): Point {
    return [this.inner.panX, this.inner.panY];
  }

  get dpr(): number {
    return this.inner.dpr;
  }

  panBy(dx: number, dy: number): void {
    this.inner.panBy(dx, dy);
  }

  /** Multiply the zoom, keeping the screen point (x, y) still. */
  zoomBy(factor: number, x: number, y: number): void {
    this.inner.zoomBy(factor, x, y);
  }

  zoomIn(): void {
    this.inner.zoomIn();
  }

  zoomOut(): void {
    this.inner.zoomOut();
  }

  /** 1 is 100%. Around the viewport's centre. */
  zoomTo(zoom: number): void {
    this.inner.zoomTo(zoom);
  }

  zoomToFit(): void {
    this.inner.zoomToFit();
  }

  setView(zoom: number, panX: number, panY: number): void {
    this.inner.setView(zoom, panX, panY);
  }

  /** The colour around the artboard, as sRGB bytes. */
  setBackdrop(r: number, g: number, b: number): void {
    this.inner.setBackdrop(r, g, b);
  }

  private view: Uint8ClampedArray<ArrayBuffer> | null = null;

  /**
   * The canvas pixels, read in place from wasm memory — nothing is copied.
   * The view is rebuilt when wasm memory grows (which detaches the old
   * buffer) or the artboard changes size.
   *
   * The bytes are premultiplied RGBA, and ImageData expects straight alpha.
   * The two are identical while every pixel is opaque, which holds because
   * every frame starts from the opaque backdrop. A transparent backdrop or
   * artboard would have to unpremultiply here.
   */
  pixels(): Uint8ClampedArray<ArrayBuffer> {
    if (!memory) throw new Error("wasm core is not loaded");
    const pointer = this.inner.pixelsPtr();
    const length = this.width * this.height * 4;
    const view = this.view;
    if (
      !view ||
      view.buffer !== memory.buffer ||
      view.byteOffset !== pointer ||
      view.length !== length
    ) {
      // Never a SharedArrayBuffer: this build has no wasm threads (GitHub
      // Pages cannot enable them), and ImageData would refuse one.
      this.view = new Uint8ClampedArray(memory.buffer as ArrayBuffer, pointer, length);
    }
    return this.view!;
  }

  // The properties panel. Values are what the user types — document units,
  // degrees — not screen pixels.

  private lastProperties: { json: string; value: Properties | null } | null = null;

  /**
   * What the panel shows for the selection, or null with nothing selected.
   * Returns the same object for as long as nothing in it changes, so a view
   * can memoise on it; it does change on every frame of a drag.
   */
  properties(): Properties | null {
    const json = this.inner.properties();
    if (this.lastProperties?.json !== json) {
      this.lastProperties = { json, value: JSON.parse(json) as Properties | null };
    }
    return this.lastProperties.value;
  }

  /**
   * Show a change without recording it: each move of a scrub or a picker
   * drag. `commitProperty` records them all as one undo step.
   */
  previewProperty(change: PropertyChange): boolean {
    return this.inner.previewProperty(JSON.stringify(change));
  }

  commitProperty(): boolean {
    return this.inner.commitProperty();
  }

  /** Drop the previews and put back what was there. True if there were any. */
  cancelProperty(): boolean {
    return this.inner.cancelProperty();
  }

  /** Change the selection as one undo step: a typed value, a picked swatch. */
  setProperty(change: PropertyChange): boolean {
    return this.inner.setProperty(JSON.stringify(change));
  }

  // Layers: each call is one undo step.

  /** False for a blank name or the one it has. */
  rename(id: string, name: string): boolean {
    return this.inner.rename(id, name);
  }

  setVisible(id: string, visible: boolean): boolean {
    return this.inner.setVisible(id, visible);
  }

  setLocked(id: string, locked: boolean): boolean {
    return this.inner.setLocked(id, locked);
  }

  /** Hide the selection, or show it if all of it is hidden. */
  toggleVisible(): boolean {
    return this.inner.toggleVisible();
  }

  /** Lock the selection, or unlock it if all of it is locked. */
  toggleLocked(): boolean {
    return this.inner.toggleLocked();
  }

  /** Move the selected layers to where they were dropped in the panel. */
  moveSelection(id: string, place: DropPlace): boolean {
    return this.inner.moveSelection(id, place);
  }

  arrange(how: Arrangement): boolean {
    return this.inner.arrange(how);
  }

  /** Put the selection in a new group, which becomes the selection. */
  group(): boolean {
    return this.inner.group();
  }

  ungroup(): boolean {
    return this.inner.ungroup();
  }

  layers(): LayerRow[] {
    return JSON.parse(this.inner.layerTree()) as LayerRow[];
  }

  /** Changes whenever `layers()` may have: cache the rows on it. */
  get layersVersion(): string {
    return this.inner.layersVersion();
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

  /** The artwork as SVG text, the size of the artboard; the caller decides where it goes. */
  exportSvg(): string {
    return this.inner.exportSvg();
  }

  /** The canvas's size in device pixels: the viewport, which its backing store must match. */
  get width(): number {
    return this.inner.width;
  }

  get height(): number {
    return this.inner.height;
  }

  /** The artboard's size in document units. It belongs to the document. */
  get artboard(): { width: number; height: number } {
    return { width: this.inner.artboardWidth, height: this.inner.artboardHeight };
  }

  /** A press or a property edit is in progress and the document holds a preview: do not save now. */
  get busy(): boolean {
    return this.inner.busy();
  }
}
