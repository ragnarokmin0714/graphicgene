/**
 * End-to-end check of the wasm boundary, with no browser involved.
 *
 * This box has no browser engine, and CI does not run one either, so this is
 * what stands in for "the app actually works": it drives the real wasm module
 * through draw -> transform -> undo -> redo -> save -> reload and asserts on
 * rendered pixels.
 *
 * It does NOT verify anything React does. That still needs a real browser.
 *
 * Run with: pnpm --filter @graphicgene/web smoke   (after pnpm build:wasm)
 */
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

const wasmDir = fileURLToPath(new URL("../src/wasm/", import.meta.url));
const { default: init, Editor } = await import(`${wasmDir}graphicgene_wasm.js`);
await init({ module_or_path: await readFile(`${wasmDir}graphicgene_wasm_bg.wasm`) });

const W = 64;
const H = 64;
const RED = new Uint8Array([255, 0, 0, 255]);
const pixel = (data, x, y) => Array.from(data.slice((y * W + x) * 4, (y * W + x) * 4 + 4));

const editor = new Editor(W, H);
const id = editor.addRect(0, 0, 20, 20, RED);

assert.deepEqual(pixel(editor.render(), 5, 5), [255, 0, 0, 255], "fill should be drawn");
assert.deepEqual(pixel(editor.render(), 40, 40), [255, 255, 255, 255], "outside stays clear");

editor.setTransform(id, new Float64Array([1, 0, 0, 1, 30, 30]));
let data = editor.render();
assert.deepEqual(pixel(data, 5, 5), [255, 255, 255, 255], "old position should be vacated");
assert.deepEqual(pixel(data, 35, 35), [255, 0, 0, 255], "new position should be filled");

assert.equal(editor.canUndo(), true);
editor.undo();
assert.deepEqual(pixel(editor.render(), 35, 35), [255, 255, 255, 255], "undo should revert");
editor.redo();
assert.deepEqual(pixel(editor.render(), 35, 35), [255, 0, 0, 255], "redo should reapply");

// The invariant the whole architecture rests on: a node's id survives a
// save/load round trip. Components and any future collaboration depend on it.
const reloaded = new Editor(W, H);
reloaded.loadJson(editor.toJson());
const layers = JSON.parse(reloaded.layerTree());
assert.ok(
  layers.some((layer) => layer.id === id),
  `NodeId ${id} must survive save/load, got ${JSON.stringify(layers.map((l) => l.id))}`,
);
assert.deepEqual(pixel(reloaded.render(), 35, 35), [255, 0, 0, 255], "reload should redraw");

// The layer panel reads rows in display order: no root row, topmost first.
const top = reloaded.addEllipse(50, 50, 4, 4, RED);
assert.deepEqual(
  JSON.parse(reloaded.layerTree()).map((row) => [row.id, row.depth]),
  [
    [top, 0],
    [id, 0],
  ],
  "layer rows should list the topmost node first and omit the root",
);

// Canvas interaction: draw, select, drag, delete — each one undo step.
const WHITE = [255, 255, 255, 255];
const canvas = new Editor(W, H);
canvas.beginCreate("rect", 10, 10, RED);
canvas.updateGesture(20, 20, false, false);
canvas.updateGesture(30, 30, false, false);
const drawn = canvas.endGesture();
assert.ok(drawn, "drawing a shape should return its id");
assert.deepEqual(pixel(canvas.render(), 20, 20), [255, 0, 0, 255], "drawn rect should render");
assert.equal(canvas.selectionCount(), 1, "a drawn shape becomes the selection");
assert.equal(JSON.parse(canvas.layerTree())[0].selected, true, "layer rows report selection");

assert.equal(canvas.selectAt(60, 60, false), "miss", "empty space selects nothing");
assert.equal(canvas.selectionCount(), 0, "clicking empty space clears the selection");
assert.equal(JSON.parse(canvas.overlay()).frame, null, "no selection, no frame");

assert.equal(canvas.selectAt(20, 20, false), "drag", "clicking the rect selects it");
assert.equal(canvas.beginMove(20, 20), true);
for (let step = 1; step <= 20; step++) canvas.updateGesture(20 + step, 20 + step, false, false);
assert.equal(JSON.parse(canvas.overlay()).gesture, "move");
canvas.endGesture();
data = canvas.render();
assert.deepEqual(pixel(data, 15, 15), WHITE, "drag should vacate the old spot");
assert.deepEqual(pixel(data, 45, 45), [255, 0, 0, 255], "drag should fill the new spot");
const corners = JSON.parse(canvas.overlay()).frame.corners;
assert.deepEqual(corners[0], [30, 30], "overlay frame should follow the drag");

canvas.undo();
assert.deepEqual(pixel(canvas.render(), 15, 15), [255, 0, 0, 255], "one undo reverts the whole drag");
canvas.redo();

assert.equal(canvas.selectAt(45, 45, true), "hit", "shift-click on a selected node deselects it");
assert.equal(canvas.selectionCount(), 0);

canvas.selectAll();
assert.equal(canvas.deleteSelection(), true);
assert.deepEqual(pixel(canvas.render(), 45, 45), WHITE, "delete removes the shape");
canvas.undo();
assert.deepEqual(pixel(canvas.render(), 45, 45), [255, 0, 0, 255], "undo restores it");

console.log("smoke: ok");
