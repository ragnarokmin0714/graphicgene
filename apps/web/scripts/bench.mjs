/**
 * Frame-cost benchmark of the wasm core, with no browser involved.
 *
 * Node runs V8, the same engine as Chrome, so these numbers track what the
 * web app pays per frame — minus the browser's own compositing. They exist to
 * compare builds against each other, not as absolute targets.
 *
 * Each scenario mirrors what the app does:
 *   drag    — one pointer move while dragging a shape: update the gesture,
 *             bring the pixels up to date, and move them to a canvas
 *   drag all — the same with every node selected: the worst case, where
 *             the damaged area is the whole artboard
 *   pan     — one frame of panning a 1440×900 viewport on a 2x screen: the
 *             canvas shifts its pixels and only the uncovered strip is drawn
 *   zoom    — one frame of zooming it: nothing to reuse, a full redraw
 *   idle    — the same frame when nothing changed (a selection click, say)
 *   hover   — one hover hit-test as the pointer crosses the artboard
 *   layers  — reading the layer panel's rows
 *   overlay — reading the selection overlay
 *
 * Run with: pnpm --filter @graphicgene/web bench   (after pnpm build:wasm)
 */
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const wasmDir = fileURLToPath(new URL("../src/wasm/", import.meta.url));
const { default: init, Editor } = await import(`${wasmDir}graphicgene_wasm.js`);
const wasm = await init({
  module_or_path: await readFile(`${wasmDir}graphicgene_wasm_bg.wasm`),
});

const W = 800;
const H = 600;
const FILL = new Uint8Array([99, 102, 241, 255]);

/** 400 rectangles and ellipses in a grid, plus 100 curvy paths on top. */
function populate(editor) {
  const ids = [];
  for (let i = 0; i < 400; i++) {
    const x = (i % 20) * 40 + 4;
    const y = Math.floor(i / 20) * 30 + 4;
    ids.push(i % 2 ? editor.addRect(x, y, 30, 20, FILL) : editor.addEllipse(x + 15, y + 10, 15, 10, FILL));
  }
  for (let j = 0; j < 100; j++) {
    const x = (j % 10) * 80 + 10;
    const y = Math.floor(j / 10) * 60 + 10;
    ids.push(
      editor.addPath(
        `M${x},${y} C${x + 20},${y - 10} ${x + 40},${y + 30} ${x + 60},${y + 10} ` +
          `C${x + 70},${y + 40} ${x + 20},${y + 50} ${x},${y + 30} Z`,
        FILL,
      ),
    );
  }
  return ids;
}

/**
 * One frame as Canvas.tsx does it: bring the pixels up to date, then put the
 * reported rects on the canvas, read in place from wasm memory. putImageData's
 * own copy is stood in for by copying the same rows into a buffer the size
 * of the canvas; the canvas shifting itself on a pan is the browser's work
 * and is not counted.
 */
const canvasBuffers = new Map();
function frame(editor) {
  const out = editor.render();
  const count = out[2];
  if (count === 0) return;
  const { width, height } = editor;
  const key = `${width}x${height}`;
  if (!canvasBuffers.has(key)) canvasBuffers.set(key, new Uint8ClampedArray(width * height * 4));
  const canvas = canvasBuffers.get(key);
  const pixels = new Uint8ClampedArray(wasm.memory.buffer, editor.pixelsPtr(), width * height * 4);
  for (let i = 0; i < count; i++) {
    const [x, y, w, h] = out.slice(3 + i * 4, 7 + i * 4);
    for (let row = y; row < y + h; row++) {
      const start = (row * width + x) * 4;
      canvas.set(pixels.subarray(start, start + w * 4), start);
    }
  }
}

function measure(label, runs, body) {
  const times = [];
  for (let i = 0; i < runs; i++) {
    const t0 = performance.now();
    body(i);
    times.push(performance.now() - t0);
  }
  times.sort((a, b) => a - b);
  const mean = times.reduce((s, t) => s + t, 0) / times.length;
  const p95 = times[Math.floor(times.length * 0.95)];
  return { label, mean, p95 };
}

const editor = new Editor(W, H);
const ids = populate(editor);
// Warm up: first render, JIT, caches.
for (let i = 0; i < 20; i++) frame(editor);

const results = [];

// A shape in the middle of the grid.
const target = ids[210];
editor.selectLayer(target, false);
const [cx, cy] = [10 * 40 + 20, 10 * 30 + 14];
const beginMove = () => editor.beginMove(cx, cy);
beginMove();
results.push(
  measure("drag (update + pixels)", 240, (i) => {
    editor.updateGesture(cx + (i % 60), cy + ((i * 3) % 40), false, false);
    frame(editor);
  }),
);
editor.endGesture();

// The worst case for partial redraws: everything moves, so the damaged area
// is the whole artboard and every item is redrawn.
editor.selectAll();
editor.beginMove(cx, cy);
results.push(
  measure("drag all (worst case)", 120, (i) => {
    editor.updateGesture(cx + (i % 30), cy + ((i * 3) % 20), false, false);
    frame(editor);
  }),
);
editor.cancelGesture();

frame(editor);
results.push(measure("idle frame", 240, () => frame(editor)));

results.push(
  measure("hover hit-test", 2000, (i) => {
    const x = (i * 37) % W;
    const y = (i * 53) % H;
    editor.hover(x, y, 4);
  }),
);

results.push(measure("layer rows", 120, () => JSON.parse(editor.layerTree())));

// A real viewport on a HiDPI screen: 1440×900 CSS pixels at 2x.
const big = new Editor(W, H);
populate(big);
big.setViewport(2880, 1800, 2);
for (let i = 0; i < 5; i++) frame(big);
results.push(
  measure("pan @2x (shift + strip)", 240, (i) => {
    big.panBy(i % 2 ? 3 : -2, 1);
    frame(big);
  }),
);
results.push(
  measure("zoom @2x (full redraw)", 60, (i) => {
    big.zoomBy(i % 2 ? 1.02 : 1 / 1.02, 720, 450);
    frame(big);
  }),
);

editor.selectLayer(target, false);
results.push(measure("overlay", 240, () => JSON.parse(editor.overlay())));

console.log(`graphicgene bench — ${ids.length} nodes, ${W}×${H}\n`);
console.log("scenario".padEnd(26) + "mean ms".padStart(10) + "p95 ms".padStart(10));
for (const { label, mean, p95 } of results) {
  console.log(label.padEnd(26) + mean.toFixed(3).padStart(10) + p95.toFixed(3).padStart(10));
}
