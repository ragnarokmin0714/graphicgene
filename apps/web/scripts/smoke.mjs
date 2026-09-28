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
assert.deepEqual([resized.artboardWidth, resized.artboardHeight], [120, 40], "loading adopts the file's artboard");
// With no viewport from a page, the canvas is the artboard at 100%.
assert.deepEqual([resized.width, resized.height], [120, 40], "headless, the canvas follows the artboard");
assert.equal(draw(resized).length, 120 * 40 * 4, "and renders at that size");

// Only what changed is redrawn, and the result matches a full redraw.
const partial = new Editor(200, 200);
const moving = partial.addRect(10, 10, 10, 10, RED);
partial.addRect(100, 100, 30, 30, RED);
// render() says: shift by (dx, dy), then put back n rects.
assert.deepEqual(Array.from(partial.render()), [0, 0, 1, 0, 0, 200, 200], "the first frame is everything");
assert.deepEqual(Array.from(partial.render()), [0, 0, 0], "an unchanged frame redraws nothing");
partial.selectLayer(moving, false);
assert.deepEqual(Array.from(partial.render()), [0, 0, 0], "selection is not pixels");
partial.setTransform(moving, new Float64Array([1, 0, 0, 1, 6, 0]));
const [, , count, , , dw, dh] = partial.render();
assert.equal(count, 1);
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

// The view: a page sets the viewport, and the artboard is fitted into it.
const BACKDROP = [235, 235, 235, 255];
const viewed = new Editor(64, 64);
viewed.addRect(0, 0, 20, 20, RED);
viewed.setViewport(128, 128, 1);
assert.equal(viewed.zoom, 0.5, "a 64px board fits a 128px viewport with 48px of padding at 50%");
assert.deepEqual([viewed.panX, viewed.panY], [48, 48], "centred");
const at = (editor, x, y) => pixel128(editor.pixelBytes(), x, y);
const pixel128 = (data, x, y) => Array.from(data.slice((y * 128 + x) * 4, (y * 128 + x) * 4 + 4));
viewed.render();
assert.deepEqual(at(viewed, 50, 50), [255, 0, 0, 255], "the rect, drawn through the view");
assert.deepEqual(at(viewed, 5, 5), BACKDROP, "the backdrop around the artboard");
assert.deepEqual(at(viewed, 70, 70), WHITE, "the artboard beyond the rect");

// Pointer input is in screen pixels: a click on the rect's screen position.
assert.equal(viewed.selectAt(52, 52, false, 4), "drag", "screen points are mapped into the document");

// A pan shifts the pixels it has and redraws only the strip it uncovers.
viewed.panBy(10, 0);
const panned = Array.from(viewed.render());
assert.deepEqual(panned.slice(0, 3), [10, 0, 1], "a pan is a shift plus one strip");
assert.deepEqual(panned.slice(3), [0, 0, 10, 128], "the strip on the left");
assert.deepEqual(at(viewed, 60, 50), [255, 0, 0, 255], "the rect moved with the view");
assert.equal(viewed.settle(), true, "shifted pixels ask for a redraw once the pan settles");
assert.deepEqual(Array.from(viewed.render()).slice(0, 7), [0, 0, 1, 0, 0, 128, 128]);
assert.equal(viewed.settle(), false, "and then they are exact");

// A zoom cannot reuse pixels: it redraws everything.
viewed.zoomBy(2, 64, 64);
assert.deepEqual(Array.from(viewed.render()).slice(0, 7), [0, 0, 1, 0, 0, 128, 128]);
const reference = new Editor(1, 1);
reference.loadJson(viewed.toJson());
reference.setViewport(128, 128, 1);
reference.setView(viewed.zoom, viewed.panX, viewed.panY);
reference.render();
assert.deepEqual(viewed.pixelBytes(), reference.pixelBytes(), "the same view draws the same pixels");

// The properties panel: document units whatever the view, colours as the
// sRGB bytes that went in, and a scrub previewed many times is one undo step.
const inspected = new Editor(W, H);
assert.equal(inspected.properties(), "null", "nothing selected");
const box = inspected.addRect(10, 10, 20, 20, RED);
inspected.selectLayer(box, false);
let props = JSON.parse(inspected.properties());
assert.deepEqual([props.x, props.y, props.width, props.height, props.rotation], [10, 10, 20, 20, 0]);
assert.deepEqual([props.opacity, props.fill, props.stroke], [1, [255, 0, 0, 255], null]);
assert.equal("strokeWidth" in props, false, "nothing is stroked, so no width");

for (const x of [12, 20, 30]) assert.equal(inspected.previewProperty(JSON.stringify({ x })), true);
assert.equal(inspected.busy(), true, "a preview is not saved");
assert.equal(inspected.commitProperty(), true);
assert.equal(inspected.busy(), false);
assert.equal(JSON.parse(inspected.properties()).x, 30);
inspected.undo();
assert.equal(JSON.parse(inspected.properties()).x, 10, "the whole scrub was one undo step");

inspected.setProperty(JSON.stringify({ strokeColor: [0, 0, 255, 255] }));
props = JSON.parse(inspected.properties());
assert.deepEqual([props.stroke, props.strokeWidth], [[0, 0, 255, 255], 1], "a new stroke is thin");
inspected.setProperty(JSON.stringify({ fill: [0, 255, 0, 128] }));
const [r, g, b] = pixel(draw(inspected), 20, 20);
assert.ok(Math.abs(r - 127) <= 1 && g === 255 && Math.abs(b - 127) <= 1, `half-green: ${[r, g, b]}`);
// Two units wide, so the stroke covers the pixels either side of the edge.
inspected.setProperty(JSON.stringify({ strokeWidth: 2 }));
assert.deepEqual(pixel(draw(inspected), 10, 20).slice(0, 3), [0, 0, 255], "the stroke, on the edge");

assert.throws(() => inspected.setProperty('{"colour": 1}'), /unknown property colour/);
assert.throws(() => inspected.setProperty('{"fill": [300, 0, 0, 255]}'), /\[r, g, b, a\]/);
assert.throws(() => inspected.setProperty('{"x": 1, "y": 2}'), /one key/);
inspected.clearSelection();
assert.equal(inspected.setProperty('{"x": 0}'), false, "nothing to apply it to");

// Layers: rename, hide, lock, reorder, group — each one undo step.
{
  const layered = new Editor(W, H);
  const [p, q, r] = [0, 20, 40].map((x) => layered.addRect(x, 0, 10, 10, RED));
  const names = () => JSON.parse(layered.layerTree()).map((row) => `${"  ".repeat(row.depth)}${row.name}`);
  layered.rename(p, "p");
  layered.rename(q, "q");
  layered.rename(r, "r");
  assert.equal(layered.rename(r, "  r "), false, "the name it has, once trimmed");
  assert.deepEqual(names(), ["r", "q", "p"], "topmost first");

  layered.selectLayer(p, false);
  assert.equal(layered.moveSelection(r, "above"), true);
  assert.deepEqual(names(), ["p", "r", "q"]);
  assert.equal(layered.arrange("backward"), true);
  assert.deepEqual(names(), ["r", "p", "q"]);
  assert.throws(() => layered.moveSelection(r, "beside"), /above, below or inside/);
  assert.throws(() => layered.arrange("up"), /forward, backward, front or back/);

  layered.selectLayer(q, true);
  assert.equal(layered.group(), true);
  assert.deepEqual(names(), ["r", "Group", "  p", "  q"], "the group takes p's place");
  assert.equal(layered.ungroup(), true);
  assert.deepEqual(names(), ["r", "p", "q"]);
  layered.undo();
  assert.deepEqual(names(), ["r", "Group", "  p", "  q"], "ungrouping was one step");

  assert.equal(layered.setVisible(r, false), true);
  assert.equal(JSON.parse(layered.layerTree())[0].visible, false);
  assert.deepEqual(pixel(draw(layered), 45, 5), WHITE, "a hidden layer is not drawn");
  layered.selectLayer(q, false);
  assert.equal(layered.setLocked(q, true), true);
  assert.equal(JSON.parse(layered.overlay()).locked, true, "the overlay says the selection is locked");
  assert.equal(layered.beginMove(25, 5), false, "and the canvas cannot move it");
}

// Clipboard: text out, text in, landing where it came from with fresh ids.
{
  const source = new Editor(W, H);
  const original = source.addRect(10, 10, 20, 20, RED);
  source.selectLayer(original, false);
  const text = source.copy();
  assert.equal(JSON.parse(text).type, "graphicgene/nodes");
  const target = new Editor(W, H);
  assert.equal(target.paste(text), true);
  assert.equal(target.paste("hello"), false, "text that is not ours");
  assert.ok(JSON.parse(target.layerTree())[0].selected, "pasted and selected");
  assert.deepEqual(pixel(draw(target), 20, 20), [255, 0, 0, 255], "where it was copied from");
  // Ids only mean something within one document: paste back into the source.
  assert.equal(source.paste(text), true);
  const [copy, first] = JSON.parse(source.layerTree());
  assert.ok(first.id === original && copy.id !== original, "a paste gets a fresh id");
  assert.equal(target.duplicate(), true);
  assert.equal(JSON.parse(target.layerTree()).length, 2);
  assert.ok(target.cut());
  assert.equal(JSON.parse(target.layerTree()).length, 1);
  target.clearSelection();
  assert.equal(target.copy(), undefined, "nothing selected, nothing copied");
}

// Image export: the artboard at a scale, as straight-alpha pixels for the
// page to encode.
{
  const exported = new Editor(W, H);
  exported.addRect(0, 0, 10, 10, RED);
  const image = exported.exportImage(2, false);
  const pixels = image.pixels();
  assert.deepEqual([image.width, image.height, pixels.length], [128, 128, 128 * 128 * 4], "twice the artboard");
  assert.ok(pixels instanceof Uint8ClampedArray, "ready for ImageData");
  const at = (x, y) => Array.from(pixels.slice((y * 128 + x) * 4, (y * 128 + x) * 4 + 4));
  assert.deepEqual(at(10, 10), [255, 0, 0, 255], "the rect, at twice its size");
  assert.deepEqual(at(30, 30), [255, 255, 255, 255], "the page");
  assert.deepEqual(exported.exportImage(1, true).pixels().slice(60 * 4, 60 * 4 + 4), new Uint8ClampedArray(4), "no page");
  image.free();
  assert.throws(() => exported.exportImage(0, false), /cannot be made/);
}

// Canvas input routed in core: the page reports presses in screen pixels
// and the tool in hand decides what they do.
{
  const routed = new Editor(W, H);
  routed.setViewport(128, 128, 1); // the 64px artboard at 50%, from (48, 48)
  const HIT = 4;
  const PICK = 6;
  const press = (x, y, grab = "") => routed.pointerDown(x, y, false, false, grab, 1, 1, HIT, PICK, RED);
  const moveTo = (x, y) => routed.pointerMove(x, y, false, false, HIT, PICK);
  routed.setTool("rect");
  press(58, 58);
  moveTo(68, 63);
  routed.pointerUp();
  assert.equal(routed.tool, "select", "a drawn shape hands back to Select");
  assert.deepEqual(JSON.parse(routed.overlay()).frame.corners[0], [58, 58], "drawn where pressed, on screen");
  assert.equal(JSON.parse(routed.layerTree()).length, 1);

  press(62, 60);
  moveTo(72, 70);
  routed.pointerUp();
  assert.deepEqual(JSON.parse(routed.overlay()).frame.corners[0], [68, 68], "a press on it drags it");
  press(78, 73, "scale");
  moveTo(88, 83);
  routed.pointerUp();
  assert.deepEqual(JSON.parse(routed.overlay()).frame.corners[2], [88, 83], "a grabbed handle scales");

  routed.doubleClick(75, 75, HIT, PICK);
  assert.equal(JSON.parse(routed.overlay()).mode, "path", "a double-click edits its points");
  routed.enter();
  assert.equal(JSON.parse(routed.overlay()).mode, null, "Enter finishes");
  routed.setTool("pen");
  routed.escape();
  assert.equal(routed.tool, "select", "Escape puts the tool down");
  routed.escape();
  assert.equal(routed.selectionCount(), 0, "then the selection");
  assert.throws(() => routed.setTool("brush"), /select, rect, ellipse or pen/);
}

console.log("smoke: ok");
