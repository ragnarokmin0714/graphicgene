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
/** Bring the pixels up to date and copy them out, for asserting on. */
const draw = (editor) => {
  editor.render();
  return editor.pixelBytes();
};

const editor = new Editor(W, H);
const id = editor.addRect(0, 0, 20, 20, RED);

assert.deepEqual(pixel(draw(editor), 5, 5), [255, 0, 0, 255], "fill should be drawn");
assert.deepEqual(pixel(draw(editor), 40, 40), [255, 255, 255, 255], "outside stays clear");

editor.setTransform(id, new Float64Array([1, 0, 0, 1, 30, 30]));
let data = draw(editor);
assert.deepEqual(pixel(data, 5, 5), [255, 255, 255, 255], "old position should be vacated");
assert.deepEqual(pixel(data, 35, 35), [255, 0, 0, 255], "new position should be filled");

assert.equal(editor.canUndo(), true);
editor.undo();
assert.deepEqual(pixel(draw(editor), 35, 35), [255, 255, 255, 255], "undo should revert");
editor.redo();
assert.deepEqual(pixel(draw(editor), 35, 35), [255, 0, 0, 255], "redo should reapply");

// The invariant the whole architecture rests on: a node's id survives a
// save/load round trip. Components and any future collaboration depend on it.
const reloaded = new Editor(W, H);
reloaded.loadJson(editor.toJson());
const layers = JSON.parse(reloaded.layerTree());
assert.ok(
  layers.some((layer) => layer.id === id),
  `NodeId ${id} must survive save/load, got ${JSON.stringify(layers.map((l) => l.id))}`,
);
assert.deepEqual(pixel(draw(reloaded), 35, 35), [255, 0, 0, 255], "reload should redraw");

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
assert.deepEqual(pixel(draw(canvas), 20, 20), [255, 0, 0, 255], "drawn rect should render");
assert.equal(canvas.selectionCount(), 1, "a drawn shape becomes the selection");
assert.equal(JSON.parse(canvas.layerTree())[0].selected, true, "layer rows report selection");

assert.equal(canvas.selectAt(60, 60, false, 4), "miss", "empty space selects nothing");
assert.equal(canvas.selectionCount(), 0, "clicking empty space clears the selection");
assert.equal(JSON.parse(canvas.overlay()).frame, null, "no selection, no frame");

assert.equal(canvas.selectAt(20, 20, false, 4), "drag", "clicking the rect selects it");
assert.equal(canvas.beginMove(20, 20), true);
for (let step = 1; step <= 20; step++) canvas.updateGesture(20 + step, 20 + step, false, false);
assert.equal(JSON.parse(canvas.overlay()).gesture, "move");
canvas.endGesture();
data = draw(canvas);
assert.deepEqual(pixel(data, 15, 15), WHITE, "drag should vacate the old spot");
assert.deepEqual(pixel(data, 45, 45), [255, 0, 0, 255], "drag should fill the new spot");
const corners = JSON.parse(canvas.overlay()).frame.corners;
assert.deepEqual(corners[0], [30, 30], "overlay frame should follow the drag");

canvas.undo();
assert.deepEqual(pixel(draw(canvas), 15, 15), [255, 0, 0, 255], "one undo reverts the whole drag");
canvas.redo();

assert.equal(canvas.selectAt(45, 45, true, 4), "hit", "shift-click on a selected node deselects it");
assert.equal(canvas.selectionCount(), 0);

canvas.selectAll();
assert.equal(canvas.deleteSelection(), true);
assert.deepEqual(pixel(draw(canvas), 45, 45), WHITE, "delete removes the shape");
canvas.undo();
assert.deepEqual(pixel(draw(canvas), 45, 45), [255, 0, 0, 255], "undo restores it");

// Pen: three clicks, then a click on the first anchor closes the path.
const TOL = 3;
const pen = new Editor(W, H);
for (const [x, y] of [
  [10, 10],
  [50, 10],
  [50, 50],
]) {
  pen.penPress(x, y, false, TOL, RED);
  assert.equal(pen.penRelease(), undefined, "a plain click does not finish the path");
}
assert.equal(JSON.parse(pen.overlay()).mode, "pen");
pen.penHover(11, 11, TOL);
assert.equal(JSON.parse(pen.overlay()).pen.closable, true, "the first anchor offers to close");
pen.penPress(11, 11, false, TOL, RED);
const penPath = pen.penRelease();
assert.ok(penPath, "closing finishes the path");
assert.equal(JSON.parse(pen.overlay()).mode, null, "the pen hands back to normal mode");
assert.equal(pen.selectionCount(), 1, "the finished path is selected");
data = draw(pen);
assert.notDeepEqual(pixel(data, 30, 10), WHITE, "pen paths are stroked");
assert.deepEqual(pixel(data, 40, 20), WHITE, "and not filled");
pen.undo();
assert.deepEqual(pixel(draw(pen), 30, 10), WHITE, "the whole pen path is one undo step");
pen.redo();

// Path editing: drag an anchor, insert one on a segment, delete it.
pen.selectLayer(penPath, false);
assert.equal(pen.beginPathEdit(), true);
let edit = JSON.parse(pen.overlay());
assert.equal(edit.mode, "path");
assert.equal(edit.path.anchors.length, 3);
assert.equal(pen.pathPress(50, 50, TOL, false), "anchor");
pen.pathDrag(60, 58, false, false);
assert.equal(pen.pathRelease(), true, "a moved anchor is an edit");
assert.deepEqual(JSON.parse(pen.overlay()).path.anchors[2].at, [60, 58]);
pen.undo();
edit = JSON.parse(pen.overlay());
assert.equal(edit.mode, "path", "undo keeps path editing on");
assert.deepEqual(edit.path.anchors[2].at, [50, 50], "undo puts the anchor back");

assert.equal(pen.pathPress(30, 10, TOL, false), "segment");
pen.pathRelease();
assert.equal(JSON.parse(pen.overlay()).path.anchors.length, 4, "clicking a segment adds an anchor");
assert.equal(pen.deleteSelection(), true);
assert.equal(JSON.parse(pen.overlay()).path.anchors.length, 3, "Delete removes the selected anchor");
assert.equal(pen.finishMode(), true);
assert.equal(JSON.parse(pen.overlay()).mode, null);

// Export and save: SVG text for the artwork, and a project file without the
// detached nodes that undo keeps alive.
const svg = pen.exportSvg();
assert.match(svg, /^<svg xmlns="http:\/\/www.w3.org\/2000\/svg" width="64" height="64"/);
assert.match(svg, /<path data-name="Path" d="M10,10 L50,10 L50,50 Z" fill="none" stroke="#ff0000" stroke-width="2"\/>/);
assert.equal(pen.busy(), false);

// The artboard belongs to the document: a loaded file brings its size along.
const wide = new Editor(120, 40);
wide.addRect(0, 0, 10, 10, RED);
const resized = new Editor(W, H);
resized.loadJson(wide.toJson());
assert.deepEqual([resized.width, resized.height], [120, 40], "loading adopts the file's artboard");
assert.equal(draw(resized).length, 120 * 40 * 4, "and renders at that size");

// Only what changed is redrawn, and the result matches a full redraw.
const partial = new Editor(200, 200);
const moving = partial.addRect(10, 10, 10, 10, RED);
partial.addRect(100, 100, 30, 30, RED);
assert.deepEqual(Array.from(partial.render()), [0, 0, 200, 200], "the first frame is everything");
assert.deepEqual(Array.from(partial.render()), [0, 0, 0, 0], "an unchanged frame redraws nothing");
partial.selectLayer(moving, false);
assert.deepEqual(Array.from(partial.render()), [0, 0, 0, 0], "selection is not pixels");
partial.setTransform(moving, new Float64Array([1, 0, 0, 1, 6, 0]));
const [, , dw, dh] = partial.render();
assert.ok(dw > 0 && dw < 30 && dh > 0 && dh < 20, `a small move redraws a small area, got ${dw}x${dh}`);
const fresh = new Editor(200, 200);
fresh.loadJson(partial.toJson());
fresh.render();
assert.deepEqual(partial.pixelBytes(), fresh.pixelBytes(), "partial redraws equal a full one");
const bloated = new Editor(W, H);
for (let i = 0; i < 5; i++) {
  bloated.addRect(0, 0, 10, 10, RED);
  bloated.undo();
}
// slotmap keeps vacated slots (as null) so ids keep their versions; count the live ones.
const saved = JSON.parse(bloated.toJson()).document.nodes.filter((slot) => slot.value !== null);
assert.equal(saved.length, 1, "undone inserts are not saved, only the root");

console.log("smoke: ok");
