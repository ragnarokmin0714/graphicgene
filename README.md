# graphicgene

A vector graphics editor for the web, built on a Rust core compiled to
WebAssembly.

It is designed so that raster editing (Photoshop-like) and UI design
(Figma-like) can be added later without rewriting the core: the document
model, the colour pipeline and the render boundary are already shaped for
them.

**Try it:** <https://ragnarokmin0714.github.io/graphicgene/>

> **Status: v0.1 preview, v0.2 under way.** Draw rectangles, ellipses and
> bezier paths; select, move, scale and rotate them; edit any path point by
> point; set position, size, rotation, opacity, fill and stroke in a
> properties panel; rename, hide, lock, reorder and group layers; copy,
> paste and duplicate; zoom and pan, sharp on HiDPI screens; undo
> everything.
> Work autosaves in the browser, project files can be downloaded and
> reopened, and the artwork exports as SVG. What comes next is in
> [`ROADMAP.md`](ROADMAP.md).

## Using it

| | |
|---|---|
| `V` `R` `O` `P` | Select, Rectangle, Ellipse, Pen |
| Drag with a shape tool | Draw it — `Shift` for equal sides, `Alt` from the centre |
| Pen | Click for a corner, drag for a curve; click the first point to close, `Enter` to finish |
| Double-click a path | Edit its points: drag points and handles, click a segment to add a point, double-click a point for corner/curve |
| Handles on a selection | Scale (`Shift` keeps proportions, `Alt` from the centre); just outside a corner rotates (`Shift` snaps to 15°) |
| Arrow keys | Nudge 1px, 10px with `Shift` |
| Properties panel | Type a value and press `Enter`; `↑` `↓` step it (`Shift` for 10); drag a field's label sideways to scrub; `Esc` puts back what was being typed or scrubbed |
| Colour swatch | Opens a picker: drag in the square or along the hue and opacity strips, or pick a preset |
| Layers panel | Double-click a name to rename it; the eye and lock on a row hide and lock it; drag rows to reorder, onto the middle of a group to put them inside; right-click for more |
| `Ctrl/⌘ C` `X` `V`, `Ctrl/⌘ D` | Copy, cut, paste — in place, into this document or another tab's — and duplicate |
| `Ctrl/⌘ G`, `Ctrl/⌘ Shift G` | Group, ungroup |
| `Ctrl/⌘ ]` `[`, with `Shift` | Bring forward, send backward — with `Shift`, to the front or back |
| `Ctrl/⌘ Shift H`, `Ctrl/⌘ Shift L` | Hide or show, lock or unlock the selection |
| Scroll, `Space`+drag, middle-drag | Pan |
| `Ctrl/⌘`+scroll, pinch | Zoom about the pointer |
| `Ctrl/⌘ +` `−` `0`, `Shift 1` | Zoom in, out, to 100%, to fit the artboard |
| `Ctrl/⌘ Z`, `Ctrl/⌘ Shift Z` | Undo, redo |
| `Ctrl/⌘ O`, `Ctrl/⌘ Shift E` | Open a project file, export SVG |

## How it is built

```
document (arena + change log) → layout pass → RenderScene → Renderer → pixels
          ▲                                     (updated in place;    (only the
    editing session                              reports damage)      damaged rect)
          ▲
   platform shell (wasm today, desktop later) ◄── React UI
```

| Crate | Responsibility |
|---|---|
| `graphicgene-core` | The document, commands and undo, and the editing session — selection, drags, the pen, path editing, and the rules tying them together. Performs no IO and touches no platform API. |
| `graphicgene-render` | The render scene, updated in place from the document's change log; the `Renderer` trait; a CPU backend on `tiny-skia`. |
| `graphicgene-wasm` | A thin shell over the session: ids as strings, results as JSON, pixels read in place from wasm memory. |

The UI (`apps/web`) is React, Tailwind and shadcn, and holds **no** document
state: the document lives in Rust, and React renders a view of it and sends
commands back. A desktop shell would drive the same session.

Only what changes is redrawn, and pixels never cross the wasm boundary by
copy. With 500 shapes on the artboard, a frame of dragging one of them costs
about 0.1 ms of work in the core, and panning a 1440×900 view on a 2× screen
about 0.7 ms — a pan shifts the pixels it already has.

[`CLAUDE.md`](CLAUDE.md) records the decisions behind all this — which are
load-bearing, and why — and is worth reading before changing anything
structural.

## Getting started

Requires Rust, [`wasm-pack`](https://rustwasm.github.io/wasm-pack/), Node 24+
and pnpm.

```sh
pnpm install
pnpm dev      # builds the wasm package, then starts Vite
```

## Checks

None of these needs a browser, which is how CI runs them:

```sh
cargo test --workspace                                  # core and renderer
cargo clippy --workspace --all-targets -- -D warnings
pnpm smoke    # the real wasm module end to end, asserting on pixels
pnpm ui       # the React app in jsdom, against the real core
pnpm bench    # per-frame costs, to compare before and after a change
pnpm build    # production build
```

`pnpm ui` drives the app with synthetic pointer and keyboard events and even
checks that the canvas matches a full redraw, but it cannot judge layout,
feel or looks — changes to those still need a look in a real browser.

## Licence

Dual-licensed under MIT or Apache-2.0, at your option.
