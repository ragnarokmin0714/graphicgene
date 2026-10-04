/**
 * Browser-free UI check.
 *
 * Drives the real App — React, the wasm core, autosave — in jsdom with
 * synthetic pointer and keyboard events, and asserts on what a user would
 * see: the overlay, the layer panel, the status bar, the canvas pixels,
 * downloads and IndexedDB.
 *
 * Neither this box nor CI has a browser engine; this is what stands in for
 * one. It cannot see real layout and CSS, pointer capture, focus, IME, or
 * how anything actually looks. Those still need a person with a browser.
 *
 * Run with: pnpm --filter @graphicgene/web ui   (after pnpm build:wasm)
 */
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { JSDOM } from "jsdom";

// ---- A browser, as far as the app can tell ------------------------------------

const dom = new JSDOM('<!doctype html><html><body><div id="root"></div></body></html>', {
  pretendToBeVisual: true,
  url: "http://localhost/",
});
const { window } = dom;
const define = (key, value) =>
  Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
for (const key of [
  "window",
  "document",
  "Node",
  "NodeFilter",
  "Element",
  "HTMLElement",
  "HTMLInputElement",
  "HTMLCanvasElement",
  "SVGElement",
  "Event",
  "CustomEvent",
  "MouseEvent",
  "PointerEvent",
  "WheelEvent",
  "KeyboardEvent",
  "FocusEvent",
  "DOMRect",
  "MutationObserver",
  "getComputedStyle",
  "requestAnimationFrame",
  "cancelAnimationFrame",
  "localStorage",
]) {
  define(key, key === "window" ? window : window[key]);
}
define("IS_REACT_ACT_ENVIRONMENT", true);

// What jsdom leaves out. It does no layout, so every observed element — the
// canvas viewport, above all — reports this size, at a pixel ratio of 1.
const VIEWPORT = { width: 1000, height: 700 };
window.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} });
define("matchMedia", window.matchMedia);
class SizedResizeObserver {
  constructor(callback) {
    this.callback = callback;
  }
  observe(target) {
    const { width, height } = VIEWPORT;
    const entry = {
      target,
      contentRect: { width, height },
      devicePixelContentBoxSize: [{ inlineSize: width, blockSize: height }],
    };
    queueMicrotask(() => this.callback([entry], this));
  }
  unobserve() {}
  disconnect() {}
}
define("ResizeObserver", SizedResizeObserver);
window.ResizeObserver = SizedResizeObserver;
Object.assign(window.Element.prototype, {
  setPointerCapture() {},
  releasePointerCapture() {},
  hasPointerCapture: () => false,
});

// IndexedDB. With `window` defined, fake-indexeddb installs onto it.
await import("fake-indexeddb/auto");
define("indexedDB", window.indexedDB ?? globalThis.indexedDB);
define("IDBKeyRange", window.IDBKeyRange ?? globalThis.IDBKeyRange);

// Downloads: capture the blob instead of navigating.
const downloads = [];
URL.createObjectURL = (blob) => {
  downloads.push(blob);
  return "blob:captured";
};
URL.revokeObjectURL = () => {};
window.HTMLAnchorElement.prototype.click = function () {
  downloads.at(-1).filename = this.download;
};
// What the desktop shell was asked to save, when the App runs as the
// desktop app (the "Desktop app" section).
const shellSaves = [];

// The canvas: a 2D context whose putImageData copies into a "screen" buffer,
// cleared whenever the element changes size, as a real one is, and whose
// drawImage can shift the canvas onto itself, as a pan does.
class ImageData {
  constructor(data, width, height) {
    if (data.length !== width * height * 4) throw new RangeError("ImageData size mismatch");
    Object.assign(this, { data, width, height });
  }
}
define("ImageData", ImageData);
const screens = new WeakMap();
function screenOf(canvas) {
  let screen = screens.get(canvas);
  if (!screen || screen.width !== canvas.width || screen.height !== canvas.height) {
    screen = {
      width: canvas.width,
      height: canvas.height,
      pixels: new Uint8ClampedArray(canvas.width * canvas.height * 4),
      puts: 0,
      partialPuts: 0,
      shifts: 0,
    };
    screens.set(canvas, screen);
  }
  return screen;
}
window.HTMLCanvasElement.prototype.getContext = function () {
  const canvas = this;
  return {
    drawImage(source, dx, dy) {
      if (source !== canvas) throw new Error("only a canvas drawn onto itself is faked");
      const screen = screenOf(canvas);
      screen.shifts++;
      const { width, height, pixels } = screen;
      const before = pixels.slice();
      const span = (width - Math.abs(dx)) * 4;
      for (let y = 0; y < height; y++) {
        const from = y - dy;
        if (from < 0 || from >= height || span <= 0) continue;
        const start = (from * width + Math.max(0, -dx)) * 4;
        pixels.set(before.subarray(start, start + span), (y * width + Math.max(0, dx)) * 4);
      }
    },
    putImageData(image, dx, dy, sx = 0, sy = 0, sw = image.width, sh = image.height) {
      const screen = screenOf(canvas);
      screen.puts++;
      if (sw < image.width || sh < image.height) screen.partialPuts++;
      for (let y = sy; y < sy + sh; y++) {
        const from = (y * image.width + sx) * 4;
        screen.pixels.set(image.data.subarray(from, from + sw * 4), ((dy + y) * screen.width + dx + sx) * 4);
      }
    },
  };
};

// Encoding: jsdom has none, so a canvas "encodes" to a note of its size
// and first pixel — enough to check what reached it.
window.HTMLCanvasElement.prototype.toBlob = function (callback, type) {
  const { pixels } = screenOf(this);
  const summary = { type, width: this.width, height: this.height, first: Array.from(pixels.slice(0, 4)) };
  setTimeout(() => callback(new Blob([JSON.stringify(summary)], { type })), 0);
};

// The wasm module is fetched from a file: URL, which Node's fetch refuses,
// and fonts from paths the Vite dev server would serve: read both from disk.
const nodeFetch = globalThis.fetch;
globalThis.fetch = async (input, init) => {
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  if (url.startsWith("file:")) {
    return new Response(await readFile(fileURLToPath(url)), {
      headers: { "content-type": "application/wasm" },
    });
  }
  if (url.startsWith("/")) {
    const path = url.split("?")[0];
    const file = path.startsWith("/@fs/") ? path.slice(4) : fileURLToPath(new URL(`..${path}`, import.meta.url));
    return new Response(await readFile(file));
  }
  return nodeFetch(input, init);
};

// ---- The app ------------------------------------------------------------------

const { createServer } = await import("vite");
const server = await createServer({
  root: fileURLToPath(new URL("..", import.meta.url)),
  server: { middlewareMode: true },
  appType: "custom",
  logLevel: "error",
});
const { default: React } = await import("react");
const { createRoot } = await import("react-dom/client");
const { act } = React;

let failures = 0;
function check(ok, label) {
  console.log(`${ok ? "ok  " : "FAIL"} ${label}`);
  if (!ok) failures++;
}
function section(title) {
  console.log(`\n# ${title}`);
}

const wait = (ms) => act(() => new Promise((resolve) => setTimeout(resolve, ms)));

/**
 * Mount the app and wait until the core is up and has painted the canvas at
 * the viewport's size — and, if given, until `ready()` holds too: restoring
 * an autosave finishes later, since IndexedDB answers asynchronously.
 */
async function mount(App, ready = () => true) {
  const root = createRoot(document.getElementById("root"));
  await act(async () => root.render(React.createElement(App)));
  for (let i = 0; i < 200; i++) {
    const canvas = document.querySelector("canvas");
    const sized = canvas?.width === VIEWPORT.width && canvas.height === VIEWPORT.height;
    if (sized && screenOf(canvas).puts > 0 && ready()) break;
    await wait(10);
  }
  return root;
}

try {
  const { App } = await server.ssrLoadModule("/src/App.tsx");
  const { EditorHandle } = await server.ssrLoadModule("/src/editor.ts");
  const { resolveBackdrop } = await server.ssrLoadModule("/src/backdrop.ts");
  // Fonts the app hands its core, in order, so a reference redraw can have
  // the same ones — text is drawn from them. Kept per editor: a remounted
  // app starts with none.
  let app = { editor: null, fonts: [] };
  const addFont = EditorHandle.prototype.addFont;
  EditorHandle.prototype.addFont = function (sfnt) {
    if (app.editor !== this) app = { editor: this, fonts: [] };
    app.fonts.push(sfnt);
    return addFont.call(this, sfnt);
  };

  let root = await mount(App);
  const surface = () => document.querySelector("[data-viewport]");
  const canvas = () => document.querySelector("canvas");
  // jsdom does no layout: put the viewport at the page origin.
  const place = () => {
    surface().getBoundingClientRect = () => ({
      left: 0,
      top: 0,
      x: 0,
      y: 0,
      width: VIEWPORT.width,
      height: VIEWPORT.height,
      right: VIEWPORT.width,
      bottom: VIEWPORT.height,
    });
  };
  place();

  // The checks below speak artboard coordinates — where things are on the
  // artboard at 100% — and these map them through whatever the view is now,
  // as a user's eye does. Zoom and pan checks use screen points directly.
  const view = () => surface().dataset.view.split(" ").map(Number);
  const toScreen = (x, y) => {
    const [zoom, panX, panY] = view();
    return [x * zoom + panX, y * zoom + panY];
  };
  const fromScreen = (x, y) => {
    const [zoom, panX, panY] = view();
    return [(x - panX) / zoom, (y - panY) / zoom];
  };
  const pointerAt = (type, x, y, options = {}) =>
    act(async () => {
      surface().dispatchEvent(
        new window.PointerEvent(type, {
          bubbles: true,
          clientX: x,
          clientY: y,
          button: 0,
          pointerId: 1,
          ...options,
        }),
      );
    });
  const pointer = (type, x, y, options = {}) => pointerAt(type, ...toScreen(x, y), options);
  const click = async (x, y, options = {}) => {
    await pointer("pointerdown", x, y, options);
    await pointer("pointerup", x, y, options);
  };
  const doubleClick = (x, y) =>
    act(async () => {
      const [clientX, clientY] = toScreen(x, y);
      surface().dispatchEvent(new window.MouseEvent("dblclick", { bubbles: true, clientX, clientY }));
    });
  const wheel = async (x, y, options) => {
    await act(async () => {
      surface().dispatchEvent(
        new window.WheelEvent("wheel", { bubbles: true, cancelable: true, clientX: x, clientY: y, ...options }),
      );
    });
    // The canvas applies wheel input once per animation frame.
    await wait(40);
  };
  const drag = async (from, to, options = {}, steps = 5) => {
    await pointer("pointerdown", ...from, options);
    for (let i = 1; i <= steps; i++) {
      const at = [0, 1].map((k) => from[k] + ((to[k] - from[k]) * i) / steps);
      await pointer("pointermove", ...at, options);
    }
    await pointer("pointerup", ...to, options);
  };
  // As a browser does: at whatever has focus, through document and window,
  // and cancelable, so a control that handles a key can say so.
  const key = (name, options = {}, type = "keydown") =>
    act(async () => {
      const target = document.activeElement ?? document.body;
      target.dispatchEvent(
        new window.KeyboardEvent(type, { key: name, bubbles: true, cancelable: true, ...options }),
      );
    });
  const press = (button) => act(async () => button.click());
  const button = (label) => document.querySelector(`button[aria-label="${label}"]`);

  const screenCorners = () =>
    document
      .querySelector("svg polygon")
      ?.getAttribute("points")
      .split(" ")
      .map((p) => p.split(",").map(Number)) ?? null;
  /** The selection frame in artboard coordinates, as "x,y x,y x,y x,y". */
  const frame = () =>
    screenCorners()
      ?.map(([x, y]) => fromScreen(x, y).map((v) => Math.round(v * 1000) / 1000).join(","))
      .join(" ") ?? null;
  const corners = () => frame().split(" ").map((p) => p.split(",").map(Number));
  // The selection's size label — not the artboard's, which also reads "W × H".
  const sizeLabel = () =>
    [...surface().querySelectorAll("div.bg-primary")].find((d) => d.textContent.includes("×"))
      ?.textContent ?? null;
  const status = () => document.querySelector("footer").textContent;
  const pressed = (label) => button(label)?.getAttribute("aria-pressed");
  const layerCount = () => document.querySelectorAll("aside li").length;
  const anchorMarks = () => [...surface().querySelectorAll('svg rect[width="7"]')];

  const downloadProject = async () => {
    // In the desktop app the same button saves through the shell instead.
    if (globalThis.isTauri) {
      await press(button("Save project file as…"));
      await wait(10);
      return new TextDecoder().decode(shellSaves.at(-1));
    }
    await press(button("Download project file"));
    return downloads.at(-1).text();
  };

  /**
   * The canvas must show exactly what a full redraw of the same document
   * draws: every changed region was redrawn in the core and put on screen.
   */
  const checkScreen = async (label) => {
    // Let a pan settle: shifted pixels are only exact after that redraw.
    await wait(200);
    const text = await downloadProject();
    const [zoom, panX, panY] = view();
    const reference = await EditorHandle.create(1, 1);
    // Through the original: the reference's fonts are not the app's.
    for (const sfnt of app.fonts) addFont.call(reference, sfnt);
    reference.loadJson(text);
    reference.setViewport(canvas().width, canvas().height, 1);
    reference.setView(zoom, panX, panY);
    reference.setBackdrop(...resolveBackdrop());
    reference.render();
    const expected = reference.pixels();
    const { pixels } = screenOf(canvas());
    let differing = 0;
    for (let i = 0; i < expected.length; i++) if (pixels[i] !== expected[i]) differing++;
    check(differing === 0, `${label}: the canvas matches a full redraw (${differing} bytes differ)`);
  };

  section("Loading");
  check(!document.querySelector('[role="alert"]'), "the editor loads without an error banner");
  check(canvas().width === VIEWPORT.width, "the canvas covers the viewport, in device pixels");
  check(view().join(" ") === "1 100 50", `the artboard is centred at 100% (${view().join(" ")})`);
  check(!!button("Zoom 100%"), "and the status bar says so");
  await checkScreen("on loading");

  section("Drawing and transforming");
  await key("r");
  check(pressed("Rectangle") === "true", "R picks the rectangle tool");
  await drag([100, 100], [300, 200]);
  check(layerCount() === 1, "dragging draws one shape");
  check(pressed("Select") === "true", "the tool hands back to Select");
  check(status().includes("1 selected"), "the new shape is selected");
  check(frame() === "100,100 300,100 300,200 100,200", `its frame is the drawn rect (${frame()})`);
  check(sizeLabel() === "200 × 100", `the size label reads 200 × 100 (${sizeLabel()})`);
  check(surface().querySelectorAll("svg rect").length === 8, "a large frame has 8 handles");
  check(document.querySelector("aside li")?.dataset.selected !== undefined, "its layer row is highlighted");

  await drag([200, 150], [250, 180]);
  check(frame() === "150,130 350,130 350,230 150,230", `dragging the body moves it (${frame()})`);
  await key("z", { ctrlKey: true });
  check(frame() === "100,100 300,100 300,200 100,200", "one Ctrl+Z undoes the whole drag");
  await key("z", { ctrlKey: true, shiftKey: true });
  check(frame() === "150,130 350,130 350,230 150,230", "Ctrl+Shift+Z redoes it");
  await checkScreen("after a drag, undo and redo");
  check(screenOf(canvas()).partialPuts > 0, "drags repaint only the area that changed");

  await drag([350, 230], [400, 280]);
  check(sizeLabel() === "250 × 150", `the corner handle scales (${sizeLabel()})`);
  check(frame()?.startsWith("150,130 "), "about the opposite corner");
  await drag([400, 280], [500, 300], { shiftKey: true });
  const [w, h] = sizeLabel().split(" × ").map(Number);
  check(Math.abs(w / h - 250 / 150) < 0.01, `Shift keeps the proportions (${sizeLabel()})`);
  await key("z", { ctrlKey: true });

  await drag([408, 122], [430, 200]);
  const tilted = corners();
  check(Math.abs(tilted[0][1] - tilted[1][1]) > 1, "just outside a corner rotates");
  check(sizeLabel() === "250 × 150", `rotation keeps the size (${sizeLabel()})`);
  await checkScreen("after scaling and rotating");

  section("Selecting");
  const putsBefore = screenOf(canvas()).puts;
  await key("Escape");
  check(frame() === null && !status().includes("selected"), "Escape clears the selection");
  check(screenOf(canvas()).puts === putsBefore, "which repaints no pixels: selection is overlay only");
  await pointer("pointermove", 275, 205);
  check(!!surface().querySelector('svg path[stroke-width="1.5"]'), "hovering a shape outlines it");
  await pointer("pointermove", 780, 580);
  check(!surface().querySelector('svg path[stroke-width="1.5"]'), "the outline goes when the pointer leaves it");

  await pointer("pointerdown", 780, 580);
  await pointer("pointermove", 500, 400);
  check(!!surface().querySelector("svg rect.fill-primary\\/8"), "sweeping from empty space draws a marquee");
  await pointer("pointermove", 260, 200);
  await pointer("pointerup", 260, 200);
  check(status().includes("1 selected"), "the marquee selects what it touches");

  const before = corners();
  await key("ArrowRight", { shiftKey: true });
  check(Math.abs(corners()[0][0] - before[0][0] - 10) < 1e-6, "Shift+Arrow nudges 10px");

  await press(button("Ellipse"));
  check(pressed("Ellipse") === "true", "the dock picks the ellipse tool");
  await click(600, 50);
  check(layerCount() === 2 && sizeLabel() === "100 × 100", `a click places a 100 × 100 ellipse (${sizeLabel()})`);
  await key("r");
  await drag([50, 400], [130, 450], { shiftKey: true });
  check(sizeLabel() === "80 × 80", `Shift draws a square (${sizeLabel()})`);

  await key("a", { ctrlKey: true });
  check(status().includes("3 selected"), "Ctrl+A selects everything");
  check(surface().querySelectorAll('svg path[stroke-width="1"]').length === 3, "each selected node is outlined");
  await key("Delete");
  check(layerCount() === 0, "Delete removes the selection");
  await checkScreen("after deleting everything");
  await key("z", { ctrlKey: true });
  check(layerCount() === 3, "one undo brings all three back");

  await click(-50, -30);
  check(!status().includes("selected"), "pressing the backdrop clears the selection");
  const rows = document.querySelectorAll("aside li");
  await act(async () => rows[0].dispatchEvent(new window.MouseEvent("click", { bubbles: true })));
  await act(async () => rows[1].dispatchEvent(new window.MouseEvent("click", { bubbles: true, shiftKey: true })));
  check(status().includes("2 selected"), "layer rows select, and Shift adds");

  const settled = frame();
  const settledStatus = status();
  await pointer("pointerdown", 650, 100);
  await pointer("pointermove", 700, 500);
  const midDrag = frame();
  await key("Escape");
  await pointer("pointerup", 700, 500);
  check(midDrag !== settled && frame() === settled, "Escape during a drag puts things back");
  check(status() === settledStatus, "and keeps the selection");
  await key("z", { ctrlKey: true });
  check(layerCount() === 2, "the cancelled drag left nothing in the history");
  await checkScreen("after a cancelled drag and an undo");

  section("Pen");
  await key("Escape");
  await key("Escape");
  const layersBefore = layerCount();
  await key("p");
  check(pressed("Pen") === "true", "P picks the pen");
  check(status().includes("Click to add a point"), "a hint says how to start");
  await click(100, 300);
  await click(200, 300);
  check(status().includes("Click the first point to close"), "another says how to finish");
  await drag([250, 380], [280, 380], {}, 2);
  check(surface().querySelectorAll("svg line").length === 2, "dragging a point pulls out both handles");
  await pointer("pointermove", 180, 250);
  check(!!surface().querySelector('svg path[stroke-width="1"]'), "a preview segment follows the pointer");
  await pointer("pointermove", 101, 301);
  check(!!surface().querySelector('svg circle[r="7"]'), "hovering the first point offers to close");
  await click(101, 301);
  check(layerCount() === layersBefore + 1, "closing adds one path");
  check(pressed("Select") === "true" && status().includes("1 selected"), "which is selected, with Select back");
  await checkScreen("after drawing a pen path");

  section("Editing points");
  await click(150, 300);
  await doubleClick(150, 300);
  check(status().includes("Drag points and handles"), "double-clicking a path edits its points");
  check(frame() === null && anchorMarks().length === 3, "its three points replace the frame");
  await drag([200, 300], [200, 250]);
  const at = (y) =>
    anchorMarks().some((r) => Math.abs(fromScreen(0, +r.getAttribute("y") + 3.5)[1] - y) < 1e-6);
  check(at(250), "dragging a point moves it");
  await key("z", { ctrlKey: true });
  check(at(300) && status().includes("Drag points"), "Ctrl+Z puts it back and editing goes on");
  await click(150, 300);
  check(anchorMarks().length === 4, "clicking a segment adds a point");
  await key("Delete");
  check(anchorMarks().length === 3, "Delete removes it");
  await key("Enter");
  check(!status().includes("Drag points") && !!frame(), "Enter finishes editing");

  const layersNow = layerCount();
  await key("p");
  await click(600, 500);
  await key("Escape");
  check(layerCount() === layersNow && pressed("Select") === "true", "a one-point pen path leaves nothing behind");
  await key("p");
  await click(400, 100);
  await click(500, 120);
  await click(500, 120);
  await doubleClick(500, 120);
  check(layerCount() === layersNow + 1, "clicking the last point finishes an open path");
  check(!status().includes("Drag points"), "without the double-click starting an edit");
  await checkScreen("after editing points");

  section("Zoom and pan");
  await key("a", { ctrlKey: true });
  const moved = (from, dx, dy) =>
    screenCorners().every(([x, y], i) => Math.abs(x - from[i][0] - dx) < 1e-6 && Math.abs(y - from[i][1] - dy) < 1e-6);

  let prior = screenCorners();
  const selected = status().match(/\d+ selected/)?.[0];
  const shiftsBefore = screenOf(canvas()).shifts;
  await key(" ");
  await pointerAt("pointerdown", 500, 400);
  await pointerAt("pointermove", 520, 410);
  await pointerAt("pointermove", 540, 425);
  await pointerAt("pointerup", 540, 425);
  await key(" ", {}, "keyup");
  check(moved(prior, 40, 25), "Space-drag pans by exactly the drag");
  check(screenOf(canvas()).shifts > shiftsBefore, "shifting the canvas's own pixels, not repainting them");
  check(!!selected && status().includes(selected), `without touching the selection (${selected})`);

  prior = screenCorners();
  await wheel(500, 400, { deltaY: 30 });
  check(moved(prior, 0, -30), "the wheel pans");
  prior = screenCorners();
  await pointerAt("pointerdown", 300, 300, { button: 1 });
  await pointerAt("pointermove", 280, 300, { button: 1 });
  await pointerAt("pointerup", 280, 300, { button: 1 });
  check(moved(prior, -20, 0), "so does dragging with the middle button");
  await checkScreen("after panning");

  const [ax, ay] = screenCorners()[0];
  await wheel(ax, ay, { deltaY: -100, ctrlKey: true });
  const [bx, by] = screenCorners()[0];
  check(!!button("Zoom 116%"), `Ctrl+wheel zooms in (${document.querySelector('button[aria-label^="Zoom "]').textContent})`);
  check(Math.hypot(bx - ax, by - ay) < 1, "about the pointer: the point under it stays put");
  await key("=", { ctrlKey: true });
  check(!!button("Zoom 200%"), "Ctrl+= steps up to the next power of two");
  await key("-", { ctrlKey: true });
  check(!!button("Zoom 100%"), "Ctrl+- steps back down");
  await key("!", { code: "Digit1", shiftKey: true });
  const [fitZoom] = view();
  const board = toScreen(0, 0).concat(toScreen(800, 600));
  check(
    fitZoom > 1 && board[0] >= 0 && board[1] >= 0 && board[2] <= VIEWPORT.width && board[3] <= VIEWPORT.height,
    `Shift+1 fits the artboard (${Math.round(fitZoom * 100)}%)`,
  );
  await checkScreen("after zooming");
  await key("0", { ctrlKey: true });
  check(!!button("Zoom 100%"), "Ctrl+0 is 100%");

  section("Properties panel");
  const panel = () => document.querySelector('aside[aria-label="Properties"]');
  const field = (name) => panel().querySelector(`input[aria-label="${name}"]`);
  const value = (name) => field(name)?.value ?? null;
  const typeInto = async (name, text, commit = "Enter") => {
    const input = field(name);
    await act(async () => input.focus());
    await act(async () => {
      Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set.call(input, text);
      input.dispatchEvent(new window.Event("input", { bubbles: true }));
    });
    if (commit) await key(commit);
  };
  /** Drag a field's label sideways by `dx` screen pixels; `midway` runs before the release. */
  const scrub = async (name, dx, midway = async () => {}) => {
    const label = field(name).parentElement.querySelector("span");
    const fire = (type, x) =>
      act(async () =>
        label.dispatchEvent(
          new window.PointerEvent(type, { bubbles: true, clientX: x, button: 0, pointerId: 2 }),
        ),
      );
    await fire("pointerdown", 100);
    for (let i = 1; i <= 4; i++) await fire("pointermove", 100 + (dx * i) / 4);
    await midway();
    await fire("pointerup", 100 + dx);
  };
  const screenPixel = (x, y) => {
    const [sx, sy] = toScreen(x, y).map(Math.round);
    const { pixels, width } = screenOf(canvas());
    return Array.from(pixels.slice((sy * width + sx) * 4, (sy * width + sx) * 4 + 3));
  };
  const frameOf = () => ["X", "Y", "Width", "Height"].map(value).join(" ");

  await key("Escape");
  check(panel().textContent.includes("Nothing selected"), "with nothing selected, the panel says so");
  await key("r");
  await drag([600, 350], [700, 410]);
  check(frameOf() === "600 350 100 60", `it shows the new shape's frame (${frameOf()})`);
  check(/^[0-9A-F]{6}$/.test(value("Fill hex")) && value("Opacity") === "100", "its fill and opacity");
  check(!field("Stroke hex") && !!button("Add stroke"), "and that it has no stroke");

  await typeInto("X", "150");
  check(frame()?.startsWith("150,350 "), `typing X and Enter moves it (${frame()})`);
  check(document.activeElement === document.body, "and hands the keyboard back");
  await key("z", { ctrlKey: true });
  check(value("X") === "600", "one undo step");
  await key("z", { ctrlKey: true, shiftKey: true });

  await scrub("Width", 40);
  check(value("Width") === "140" && sizeLabel() === "140 × 60", `scrubbing W's label resizes (${sizeLabel()})`);
  check(frame()?.startsWith("150,350 "), "from the left edge");
  await key("z", { ctrlKey: true });
  check(value("Width") === "100", "and the whole scrub is one undo step");
  let midway = null;
  await scrub("Width", 40, async () => {
    midway = sizeLabel();
    await key("Escape");
  });
  check(midway === "140 × 60" && value("Width") === "100", `Escape mid-scrub puts it back (${midway})`);
  check(status().includes("1 selected"), "and keeps the selection");
  await key("z", { ctrlKey: true });
  check(value("X") === "600", "leaving nothing in the history");
  await key("z", { ctrlKey: true, shiftKey: true });

  await typeInto("Rotation", "90");
  const [c0, c1] = corners();
  check(value("Rotation") === "90" && Math.abs(c0[0] - c1[0]) < 1e-6 && c1[1] < c0[1], "rotation turns it counter-clockwise");
  await typeInto("Rotation", "0");

  await typeInto("Y", "250", null);
  await pointer("pointerdown", 780, 580);
  await pointer("pointerup", 780, 580);
  await act(async () => document.activeElement.blur());
  check(!status().includes("selected"), "a press on the canvas moves on...");
  await click(200, 280);
  check(value("Y") === "250", `...after applying what was typed, to the shape it was typed for (${frameOf()})`);

  await typeInto("Fill hex", "#0f0");
  check(value("Fill hex") === "00FF00", `hex digits set the fill (${value("Fill hex")})`);
  check(screenPixel(200, 280).join() === "0,255,0", `which the canvas shows (${screenPixel(200, 280)})`);
  await press(button("Remove fill"));
  check(!field("Fill hex") && !!button("Add fill"), "the fill can be removed");
  await key("z", { ctrlKey: true });
  check(value("Fill hex") === "00FF00", "and undo brings it back");

  await press(button("Add stroke"));
  check(value("Stroke hex") === "000000" && value("Stroke width") === "1", "Add stroke gives a thin black one");
  await typeInto("Stroke width", "4");
  await typeInto("Opacity", "50");
  check(value("Stroke width") === "4" && value("Opacity") === "50", "stroke width and opacity take typed values");
  await key("z", { ctrlKey: true });
  await key("z", { ctrlKey: true });
  check(value("Opacity") === "100" && value("Stroke width") === "1", "each one undo step");
  await checkScreen("after property edits");

  await press(button("Fill colour"));
  const picker = document.querySelector('[data-slot="popover-content"]');
  check(!!picker, "the swatch opens a colour picker");
  const square = picker.querySelector('[aria-label="Fill saturation and brightness"]');
  square.getBoundingClientRect = () => ({ left: 0, top: 0, width: 100, height: 100, right: 100, bottom: 100 });
  const firePicker = (type, x, y) =>
    act(async () =>
      square.dispatchEvent(new window.PointerEvent(type, { bubbles: true, clientX: x, clientY: y, button: 0, pointerId: 3 })),
    );
  await firePicker("pointerdown", 50, 50);
  check(value("Fill hex") === "408040", `dragging in the square previews (${value("Fill hex")})`);
  await firePicker("pointermove", 0, 0);
  await firePicker("pointerup", 0, 0);
  check(value("Fill hex") === "FFFFFF", `and releasing keeps it (${value("Fill hex")})`);
  await key("z", { ctrlKey: true });
  check(value("Fill hex") === "00FF00", "the whole picker drag is one undo step");
  await firePicker("pointerdown", 0, 100);
  check(value("Fill hex") === "000000", "pressing in the square previews at once");
  await key("Escape");
  await firePicker("pointerup", 0, 100);
  check(value("Fill hex") === "00FF00", "Escape mid-drag puts the colour back");
  check(!!document.querySelector('[data-slot="popover-content"]'), "and leaves the picker open");
  await key("Escape");
  check(!document.querySelector('[data-slot="popover-content"]'), "a second Escape closes it");
  check(status().includes("1 selected"), "without deselecting");
  await checkScreen("after the colour picker");

  section("Layers panel");
  const layerRows = () => [...document.querySelectorAll('aside[aria-label="Layers"] li')];
  const rowNames = (n = 3) => layerRows().slice(0, n).map((li) => li.textContent).join(",");
  const rowOf = (name) => layerRows().find((li) => li.textContent === name);
  const fire = (target, event) => act(async () => target.dispatchEvent(event));
  const clickRow = (name, shiftKey = false) =>
    fire(rowOf(name), new window.MouseEvent("click", { bubbles: true, shiftKey }));
  const renameRow = async (li, name) => {
    await fire(li, new window.MouseEvent("dblclick", { bubbles: true }));
    li.querySelector("input").value = name;
    await key("Enter");
  };
  for (const [i, name] of ["Alpha", "Beta", "Gamma"].entries()) await renameRow(layerRows()[i], name);
  check(rowNames() === "Alpha,Beta,Gamma", `double-clicking a row renames it (${rowNames()})`);
  await key("z", { ctrlKey: true });
  check(!rowNames().endsWith("Gamma"), "one undo step");
  await key("z", { ctrlKey: true, shiftKey: true });

  // Alpha is the green rect the properties panel left at (150, 250).
  await press(rowOf("Alpha").querySelector('button[aria-label="Hide Alpha"]'));
  check(screenPixel(200, 280).join() !== "0,255,0", "the eye hides a layer");
  check(
    rowOf("Alpha").querySelector('button[aria-label="Show Alpha"]')?.getAttribute("aria-pressed") === "true",
    "and stays on its row while it is off",
  );
  await press(rowOf("Alpha").querySelector('button[aria-label="Show Alpha"]'));
  check(screenPixel(200, 280).join() === "0,255,0", "and shows it again");

  await clickRow("Alpha");
  const handleCount = () => surface().querySelectorAll('svg rect[rx="1.5"]').length;
  check(handleCount() > 0, "a selected layer has handles");
  await press(rowOf("Alpha").querySelector('button[aria-label="Lock Alpha"]'));
  check(
    handleCount() === 0 && surface().querySelector("svg polygon")?.getAttribute("stroke-dasharray") === "4 3",
    "locked, its frame is dashed and has no handles",
  );
  await drag([200, 280], [260, 330]);
  await clickRow("Alpha");
  check(frameOf() === "150 250 100 60", `and a drag on the canvas cannot move it (${frameOf()})`);
  await press(rowOf("Alpha").querySelector('button[aria-label="Unlock Alpha"]'));

  // jsdom does no layout: stack the rows 28px apart from y = 100.
  const placeRows = () =>
    layerRows().forEach((li, i) => {
      li.getBoundingClientRect = () => ({ top: 100 + i * 28, bottom: 128 + i * 28, height: 28, left: 0, right: 240, width: 240 });
    });
  const rowY = (i, f) => 100 + i * 28 + 28 * f;
  const list = () => document.querySelector('aside[aria-label="Layers"] ul');
  const pointerOn = (target, type, y) =>
    fire(target, new window.PointerEvent(type, { bubbles: true, clientY: y, button: 0, pointerId: 4 }));
  await clickRow("Beta");
  placeRows();
  await pointerOn(rowOf("Alpha"), "pointerdown", rowY(0, 0.5));
  await pointerOn(list(), "pointermove", rowY(1, 0.5));
  await pointerOn(list(), "pointermove", rowY(2, 0.8));
  check(rowOf("Gamma").dataset.drop === "below", "dragging a row marks where it would land");
  check(status().includes("1 selected") && rowOf("Alpha").dataset.selected !== undefined, "and drags it alone, selected");
  await pointerOn(list(), "pointerup", rowY(2, 0.8));
  check(rowNames() === "Beta,Gamma,Alpha", `dropping moves it there (${rowNames()})`);
  check(!document.querySelector("li[data-drop]"), "and the mark goes");
  await checkScreen("after reordering layers");
  await key("z", { ctrlKey: true });
  check(rowNames() === "Alpha,Beta,Gamma", "one undo step");

  await pointerOn(rowOf("Beta"), "pointerdown", rowY(1, 0.5));
  await pointerOn(list(), "pointermove", rowY(4, 0.5));
  await key("Escape");
  await pointerOn(list(), "pointerup", rowY(4, 0.5));
  check(rowNames() === "Alpha,Beta,Gamma" && status().includes("1 selected"), "Escape mid-drag drops nothing");

  await clickRow("Beta");
  await clickRow("Gamma", true);
  await key("g", { ctrlKey: true });
  check(rowNames(4) === "Alpha,Group,Beta,Gamma", `Ctrl+G groups the selection (${rowNames(4)})`);
  check(rowOf("Beta").style.paddingLeft === "20px", "which is indented under the group");
  await checkScreen("after grouping");
  await key("g", { ctrlKey: true, shiftKey: true });
  check(rowNames() === "Alpha,Beta,Gamma" && rowOf("Beta").style.paddingLeft === "8px", "Ctrl+Shift+G ungroups it");

  await key("]", { ctrlKey: true, code: "BracketRight" });
  check(rowNames() === "Beta,Gamma,Alpha", `Ctrl+] brings the selection forward (${rowNames()})`);
  await key("[", { ctrlKey: true, code: "BracketLeft" });
  check(rowNames() === "Alpha,Beta,Gamma", "Ctrl+[ sends it back");

  await fire(rowOf("Alpha"), new window.MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
  const hideItem = [...document.querySelectorAll('[role="menuitem"]')].find((i) => i.textContent.startsWith("Hide"));
  check(!!hideItem && rowOf("Alpha").dataset.selected !== undefined, "right-clicking a row selects it and opens its menu");
  await press(hideItem);
  check(!!rowOf("Alpha").querySelector('button[aria-label="Show Alpha"]'), "whose items act on it");
  await key("z", { ctrlKey: true });
  await checkScreen("after the layer panel");

  section("Clipboard");
  // A browser's clipboard event, with the data jsdom leaves out.
  const clipboardEvent = (type, text) => {
    const event = new window.Event(type, { bubbles: true, cancelable: true });
    const data = text === undefined ? {} : { "text/plain": text };
    event.clipboardData = {
      setData: (kind, value) => void (data[kind] = value),
      getData: (kind) => data[kind] ?? "",
    };
    return { event, data };
  };
  const countBefore = layerCount();
  await clickRow("Alpha");
  const copied = clipboardEvent("copy");
  await fire(document.body, copied.event);
  const text = copied.data["text/plain"];
  check(copied.event.defaultPrevented && JSON.parse(text).type === "graphicgene/nodes", "copying puts the selection on the clipboard");
  await fire(document.body, clipboardEvent("paste", text).event);
  check(layerCount() === countBefore + 1 && rowNames(2) === "Alpha,Alpha", "pasting adds it on top");
  check(frameOf() === "150 250 100 60" && status().includes("1 selected"), "where it was copied from, selected");
  await key("z", { ctrlKey: true });
  check(layerCount() === countBefore, "one undo step");

  await clickRow("Alpha");
  await key("d", { ctrlKey: true });
  check(layerCount() === countBefore + 1 && rowNames(2) === "Alpha,Alpha", "Ctrl+D duplicates in place");
  await key("z", { ctrlKey: true });

  await clickRow("Alpha");
  const cut = clipboardEvent("cut");
  await fire(document.body, cut.event);
  check(layerCount() === countBefore - 1 && !!cut.data["text/plain"], "cutting copies and deletes");
  await fire(document.body, clipboardEvent("paste", cut.data["text/plain"]).event);
  check(layerCount() === countBefore && rowNames(1) === "Alpha", "and it pastes back");

  const foreign = clipboardEvent("paste", "just some words");
  await fire(document.body, foreign.event);
  check(
    !foreign.event.defaultPrevented && layerCount() === countBefore && !document.querySelector('[role="alert"]'),
    "pasting anything else does nothing, quietly",
  );
  await act(async () => field("X").focus());
  const inField = clipboardEvent("copy");
  await fire(field("X"), inField.event);
  check(!inField.event.defaultPrevented && !inField.data["text/plain"], "a text field keeps its own clipboard");
  await act(async () => field("X").blur());
  await checkScreen("after the clipboard");

  section("Text");
  await key("Escape");
  await key("Escape");
  await key("t");
  check(pressed("Text") === "true" && status().includes("Click to add text"), "T picks the text tool");
  const textLayersBefore = layerCount();
  await click(450, 500);
  const typing = () => document.querySelector('textarea[aria-label="Text"]');
  check(!!typing() && document.activeElement === typing(), "a click opens a text field, focused");
  check(status().includes("Esc or click outside to finish"), "and says how to finish");
  const type = (text) =>
    act(async () => {
      Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value").set.call(typing(), text);
      typing().dispatchEvent(new window.Event("input", { bubbles: true }));
    });
  await type("Hi 中");
  await wait(20);
  // The fonts arrive asynchronously: the Latin slices at start-up, the
  // slice with 中 once the core says it is missing.
  await wait(300);
  const inked = () => {
    let dark = 0;
    for (let y = 505; y < 530; y += 1) for (let x = 452; x < 500; x += 1) if (screenPixel(x, y)[1] < 128) dark++;
    return dark;
  };
  check(inked() > 20, `what is typed is drawn on the canvas as it is typed (${inked()} dark pixels)`);
  check(app.fonts.length === 3, `the Latin slices, then the one with 中 (${app.fonts.length} fonts)`);
  check(layerCount() === textLayersBefore + 1, "its layer row is there while typing");
  await checkScreen("while typing");
  await key("Escape");
  check(!typing() && rowNames(1) === "Hi 中", `Escape finishes: a layer named after its text (${rowNames(1)})`);
  check(pressed("Select") === "true", "and puts the text tool down");
  check(layerRows()[0].querySelector("svg.lucide-type") !== null, "its row has the text icon");
  await key("z", { ctrlKey: true });
  check(layerCount() === textLayersBefore, "one undo takes the whole text away");
  await key("z", { ctrlKey: true, shiftKey: true });

  await clickRow("Hi 中");
  check(value("Font size") === "24" && panel().textContent.includes("Noto Sans TC"), "the panel shows its font and size");
  const narrow = frameOf();
  await typeInto("Font size", "48");
  check(value("Font size") === "48" && frameOf() !== narrow, `a new size lays it out again (${frameOf()})`);
  await press(button("Align centre"));
  check(button("Align centre").getAttribute("aria-pressed") === "true", "alignment is set from the panel");
  await checkScreen("after restyling text");

  await doubleClick(460, 520);
  check(typing()?.value === "Hi 中", "a double-click types into it again");
  await type("Hi 中文");
  await wait(300);
  await key("Escape");
  check(rowNames(1) === "Hi 中文", `and the name follows the text (${rowNames(1)})`);
  await checkScreen("after editing text");

  section("Files");
  // Radix opens a menu on pointerdown, not on click.
  const exportAs = async (label) => {
    const trigger = [...document.querySelectorAll("header button")].find((b) => b.textContent === "Export");
    await fire(trigger, new window.PointerEvent("pointerdown", { bubbles: true, button: 0, pointerId: 5 }));
    // An item's text runs on into its shortcut: "SVGCtrl Shift E", "PNG2×".
    await press([...document.querySelectorAll('[role="menuitem"]')].find((item) => item.textContent.startsWith(label)));
  };
  await exportAs("SVG");
  const svg = downloads.at(-1);
  check(svg.filename === "graphicgene.svg" && svg.type === "image/svg+xml", "Export → SVG downloads graphicgene.svg");
  const parsed = new window.DOMParser().parseFromString(await svg.text(), "image/svg+xml");
  check(!parsed.querySelector("parsererror"), "which is well-formed");
  check(parsed.querySelectorAll("path").length === layerCount(), "with one <path> per layer");
  check(parsed.documentElement.getAttribute("width") === "800", "and the artboard's size");

  await exportAs("PNG2×");
  await wait(20);
  const png = downloads.at(-1);
  const encoded = JSON.parse(await png.text());
  check(png.filename === "graphicgene@2x.png" && png.type === "image/png", "Export → PNG 2× downloads graphicgene@2x.png");
  check(
    encoded.width === 1600 && encoded.height === 1200 && encoded.first.join() === "255,255,255,255",
    `encoded by the browser at twice the artboard's size, page and all (${encoded.width} × ${encoded.height})`,
  );

  const project = await downloadProject();
  check(
    downloads.at(-1).filename === "graphicgene-project.json" && JSON.parse(project).version === 2,
    "the project downloads as versioned JSON",
  );

  await wait(1000);
  check(/Saved \d/.test(status()), `autosave reports in the status bar (${status()})`);
  const savedLayers = layerCount();
  await act(async () => root.unmount());
  root = await mount(App, () => status().includes("Restored"));
  place();
  check(layerCount() === savedLayers, `reopening restores the autosave (${layerCount()} of ${savedLayers})`);
  check(status().includes("Restored your last session"), "and says so");
  await checkScreen("after a reload");

  const input = document.querySelector('input[type="file"]');
  const open = async (file) => {
    Object.defineProperty(input, "files", { value: [file], configurable: true });
    await act(async () => input.dispatchEvent(new window.Event("change", { bubbles: true })));
    await wait(50);
  };
  const small = JSON.parse(project);
  small.document.artboard = { width: 320, height: 200 };
  await open(new File([JSON.stringify(small)], "small.json", { type: "application/json" }));
  check(status().includes("Opened small.json"), "opening a file loads it");
  check(status().includes("320 × 200 px"), "the file brings its artboard size");
  check(view().join(" ") === "1 340 250", `which is fitted into the view anew (${view().join(" ")})`);
  await checkScreen("after opening a smaller artboard");

  await open(new File(["{ not json"], "broken.json"));
  check(!!document.querySelector('[role="alert"]'), "a broken file shows an error");
  check(status().includes("320 × 200 px"), "and leaves the document alone");
  await press(document.querySelector('[role="alert"] button'));
  check(!document.querySelector('[role="alert"]'), "the error can be dismissed");
  await act(async () => root.unmount());

  section("Desktop app");
  // The same App inside the desktop shell: Tauri marks the page and bridges
  // `invoke` to the Rust commands in apps/desktop. This stands in for those
  // commands — a file on disk and the system's dialogs — and records calls.
  const calls = [];
  const disk = { autosave: project, open: null, saveAs: (name) => name };
  const decoder = new TextDecoder();
  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd, args, options) => {
      calls.push({ cmd, args, options });
      switch (cmd) {
        case "read_autosave":
          return disk.autosave;
        case "write_autosave":
          disk.autosave = args.json;
          return null;
        case "open_project":
          return disk.open;
        case "save_file":
          shellSaves.push(args);
          return disk.saveAs(decodeURIComponent(options.headers["x-file-name"]));
        default:
          throw new Error(`no command ${cmd}`);
      }
    },
  };
  globalThis.isTauri = true;
  const called = (cmd) => calls.filter((call) => call.cmd === cmd);
  const downloadsBefore = downloads.length;

  root = await mount(App, () => status().includes("Restored"));
  place();
  check(called("read_autosave").length === 1, "restoring reads the autosave file, not IndexedDB");
  check(layerCount() === savedLayers, `and brings back its layers (${layerCount()} of ${savedLayers})`);
  check(
    !!button("Save project file as…") && !button("Download project file"),
    "the header saves rather than downloads",
  );

  await press(button("Save project file as…"));
  await wait(20);
  const saved = called("save_file").at(-1);
  check(
    decodeURIComponent(saved.options.headers["x-file-name"]) === "graphicgene-project.json",
    "saving suggests the project file's name",
  );
  check(
    saved.args instanceof Uint8Array && JSON.parse(decoder.decode(saved.args)).version === 2,
    "and sends the project as raw bytes",
  );
  check(status().includes("Saved graphicgene-project.json"), `the status bar says where it went (${status()})`);

  disk.saveAs = () => null;
  const beforeCancel = status();
  await press(button("Save project file as…"));
  await wait(20);
  check(status() === beforeCancel, "cancelling the save dialog changes nothing");

  await exportAs("SVG");
  await wait(20);
  const svgCall = called("save_file").at(-1);
  check(
    decodeURIComponent(svgCall.options.headers["x-file-name"]) === "graphicgene.svg" &&
      decoder.decode(svgCall.args).startsWith("<svg"),
    "SVG export goes through the save dialog too",
  );
  check(downloads.length === downloadsBefore, "and nothing is downloaded the browser's way");

  const autosavedBefore = disk.autosave;
  await key("r");
  await drag([600, 450], [660, 500]);
  await wait(1000);
  check(
    called("write_autosave").length > 0 && disk.autosave !== autosavedBefore,
    "an edit autosaves to the file",
  );
  check(/Saved \d/.test(status()), `and reports it (${status()})`);

  disk.open = { name: "small.json", text: JSON.stringify(small) };
  await press(button("Open project file"));
  await wait(50);
  check(called("open_project").length === 1, "opening asks the desktop's dialog");
  check(status().includes("Opened small.json") && status().includes("320 × 200 px"), "and loads what it picked");

  disk.open = null;
  const beforeOpenCancel = status();
  await press(button("Open project file"));
  await wait(50);
  check(status() === beforeOpenCancel && !document.querySelector('[role="alert"]'), "cancelling it changes nothing");
  await checkScreen("in the desktop app");

  await act(async () => root.unmount());
  delete globalThis.isTauri;
  delete window.__TAURI_INTERNALS__;
} finally {
  await server.close();
}

console.log(failures ? `\n${failures} check(s) failed` : "\nui: ok");
process.exit(failures ? 1 : 0);
