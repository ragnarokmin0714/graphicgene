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

console.log("smoke: ok");
