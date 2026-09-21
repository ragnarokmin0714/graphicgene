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
  name: string;
  kind: "group" | "vector";
  visible: boolean;
  locked: boolean;
  opacity: number;
};

export type Rgba = [number, number, number, number];

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
