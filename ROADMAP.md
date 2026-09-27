# Roadmap

Milestones are ordered by what unblocks what. Each item says what it needs
and which existing decision it leans on — the point of the architecture is
that most of these are additions, not rewrites.

## v0.1 — the vector core · feature-complete

Draw rectangles, ellipses and bezier paths; select, move, scale and rotate
them; edit any path point by point; undo everything; autosave in the browser,
download and reopen project files, export SVG.

Left before it counts as done: a hands-on browser pass over everything, and
the GitHub Pages deploy.

## v0.2 — a real editing surface

The things anyone misses in their first five minutes.

| Item | What it needs |
|---|---|
| **Zoom and pan, sharp on HiDPI** | A view transform from document space to device pixels. The canvas becomes the whole stage, with the artboard drawn inside it; the pixmap is sized in device pixels (zoom × `devicePixelRatio`), which also ends the upscaling blur on 2× screens. Damage rects and the overlay go through the same transform. Pick tolerances are already screen distances passed in by the shell, so they only need dividing by the zoom. |
| **Properties panel** | Fill, stroke colour and width, opacity; X / Y / W / H / rotation fields. Needs `SetStroke` and `SetLocked` commands, a colour picker that converts 8-bit sRGB to the linear model, and a preview-then-commit path for continuous controls — a slider dragged across fifty values is one undo step, as a canvas drag already is. |
| **Layer panel operations** | Rename (the first text input; shortcuts already ignore text fields), visibility and lock toggles, drag to reorder (a move command: detach plus attach in one batch), group and ungroup (baking transforms in and out). |
| **Clipboard and duplicate** | Nodes serialized to JSON on the clipboard, pasted with fresh ids. |
| **PNG export** | A tiny-skia render at a chosen scale. Core returns the bytes; the app layer saves them. |
| **Tool routing in core** | Which core call a press goes to — pen, path edit or a gesture — is still decided in `Stage.tsx`. Moving it into the session as pointer down / move / up per tool means a native shell gets it for free. |

## v0.3 — text

Shaping with `cosmic-text` / `rustybuzz`, font loading (bytes from the app
layer, since core does no IO), a `Text` node kind, and text converted to paths
on export. Editing goes through a DOM input over the canvas, so Chinese and
other IME input works — the main reason the UI is React and not a Rust
toolkit.

## v0.4 — desktop

The test of the architecture, in two steps:

1. **The same web app in a Tauri webview.** Only storage changes: files on
   disk instead of IndexedDB. If the IO-boundary rule held, core does not
   change at all.
2. **The core running natively**, behind Tauri commands, once threads or a
   GPU renderer are worth it. The session API is already shell-agnostic;
   this is a new shell, not a new core.

## Later

| Direction | What it needs |
|---|---|
| Boolean operations | Path intersection — numerically the hardest vector feature. Evaluate the maintained Rust implementations (such as `linesweeper`) before writing one. |
| GPU renderer | A second `Renderer` (`vello` or `wgpu`). The incremental scene and damage rects already suit a GPU backend. Pin `wgpu`; it breaks its API most releases. |
| Raster layers | A `Raster` node kind, pixel buffers, real blend modes and linear compositing (below). Reachable only because colour is linear `f32` in the model. |
| Components and auto layout | Node references with overrides, on the stable `NodeId`s; auto layout fills in the layout pass that already runs every frame. |
| Collaboration | A backend and a conflict model chosen then (a tree CRDT, or a server sequencer). The journal gives undo and replay, not merging. |

## Architectural debt

Known and deliberate, roughly in the order it will start to hurt.

1. **Colour is not composited in linear space yet.** The document stores
   linear `f32` colour, but tiny-skia blends and antialiases in 8-bit sRGB —
   the same gamma-incorrect result browsers produce. Correct blending needs an
   `f32` linear pipeline, from a GPU renderer or an `f32` CPU backend. The
   model already allows it; the renderer does not do it.
2. **Premultiplied pixels reach `ImageData` unconverted.** Correct while every
   pixel is opaque, which holds because the artboard is always painted white.
   A transparent artboard needs unpremultiplying on the way out.
3. **Linear scans.** Hit-testing (after a control-box broad phase) and damage
   computation visit every node. A spatial index (R-tree or BVH) is due once
   documents reach thousands of nodes; no interface assumes the scan.
4. **Structural edits rebuild the whole scene.** Attaching or detaching a
   node, or one appearing or vanishing, redraws everything. They are single
   events, so it is cheap today; very large documents may want in-place
   insertion.
5. **Unbounded undo history.** `SetPath` stores whole paths. Cap or coalesce
   it when memory shows the need.
6. **Per-frame leftovers.** Each refreshed item clones its path, the overlay
   crosses the boundary as JSON, and every draw converts its path to
   tiny-skia's type. None shows in `pnpm bench` today; measure before
   changing any of them.
7. **Anchor ids are positions.** Right for one user — path editing drops its
   point selection when points are added or removed — but collaboration will
   need anchors with stable ids.
8. **Open paths lose their outer end handles.** A `BezPath` has nowhere to
   keep them; it matters once paths can be continued from an end.

## Done in the 2026-09-27 architecture pass

- The editing session moved from the wasm crate into core, so its rules are
  tested in Rust and a desktop shell inherits them.
- Rendering became incremental: the document logs what changed, only that
  area is redrawn, and pixels are read in place from wasm memory. Dragging
  one of 500 shapes went from 9.4 ms to 0.11 ms a frame.
- The artboard size became document state, saved in the project file.
- Hit-testing gained a broad phase (9× faster hovers); the layer panel stopped
  rebuilding on every frame of a drag.
- The React app is now checked in CI, in jsdom, against the real core.
