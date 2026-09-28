# Roadmap

Milestones are ordered by what unblocks what. Each item says what it needs
and which existing decision it leans on — the point of the architecture is
that most of these are additions, not rewrites.

## v0.1 — the vector core · done, `v0.1.0`

Draw rectangles, ellipses and bezier paths; select, move, scale and rotate
them; edit any path point by point; undo everything; autosave in the browser,
download and reopen project files, export SVG.

Deployed to GitHub Pages. The hands-on browser pass over everything is
scheduled after v0.4.

## v0.2 — a real editing surface · done, `v0.2.0`

The things anyone misses in their first five minutes.

| Item | What it needs |
|---|---|
| ~~Zoom and pan, sharp on HiDPI~~ | **Done 2026-09-28.** `core::view` holds zoom and pan; the canvas covers the stage in device pixels; pans shift pixels and redraw only the new strip, then settle into an exact full redraw; wheel, pinch, Space-drag and middle-drag, zoom shortcuts and a zoom menu. |
| ~~Properties panel~~ | **Done 2026-09-28.** X / Y / W / H, rotation (counter-clockwise, as in Figma), opacity, fill and stroke colour with alpha, stroke width. Fields take typed values, arrow steps and scrubbing on their labels; a colour picker works in HSV over sRGB bytes, which the core converts to its linear model. `core::properties` previews and commits like a canvas drag, so a scrub across fifty values is one undo step and Escape puts it back. |
| ~~Layer panel operations~~ | **Done 2026-09-28.** Rename, show and hide, lock (the canvas cannot move a locked layer; the panel still can), drag to reorder or into a group, bring forward and send backward, group and ungroup. `core::layers` builds each as one batch; a layer that changes parent keeps its place on the page, and ungrouping folds the group's transform and opacity into what it held. |
| ~~Clipboard and duplicate~~ | **Done 2026-09-28.** `core::clipboard` turns the selection into marked, versioned JSON with each tree inline and its place on the page, and pastes it with fresh ids — in place, into this document or another. Duplicate copies each node right above its original. The page moves the text through the browser's copy, cut and paste events. |
| ~~PNG export~~ | **Done 2026-09-28.** `graphicgene_render::export` draws the artboard at 1×, 2× or 3×, with or without the page, as straight-alpha pixels; the browser encodes them (`canvas.toBlob`). A PNG encoder in the wasm cost 53 KB gzipped and nine crates, so tiny-skia is built without it; a native shell can encode with the `png` crate. |
| ~~Tool routing in core~~ | **Done 2026-09-28.** The session holds the tool and takes pointer down / move / up, double-clicks, Escape and Enter, routing each for the tool in hand (`session/tools.rs`). The page keeps panning, the cursor and the screen-space handle hit-test, whose result travels with the press. |

## v0.3 — text · done, `v0.3.0`

Built 2026-09-28. A `Text` node kind and a text tool (`T`): click to type,
double-click or Enter to edit, Escape or a click away to finish — one undo
step. Typing goes through a browser text field over the text box, so
Chinese and every other input method work; the core sets the text and the
canvas draws it as it is typed. Family, size, line height and alignment in
the properties panel; text exports as outlines in SVG and PNG.

The shaping plan changed on measurement: `cosmic-text`'s shaper
(harfrust with skrifa) is 309 KB of gzipped wasm and rustybuzz 232 KB, so
v0.3 shapes simply with `ttf-parser` — cmap, advances, pair kerning — at
41 KB. Fonts are Noto Sans TC and Inter from Fontsource, fetched slice by
slice as the text needs them; the core says what is missing.

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
   pixel is opaque, which holds because every frame starts from the opaque
   backdrop. A transparent backdrop or artboard needs unpremultiplying on the
   way out.
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
7. **Zooming redraws everything.** About 19 ms a frame for a 1440×900
   viewport at 2x with 500 shapes — roughly 50 fps. Showing a scaled copy of
   the last frame while a zoom gesture is under way, or the GPU renderer,
   would remove it.
8. **Anchor ids are positions.** Right for one user — path editing drops its
   point selection when points are added or removed — but collaboration will
   need anchors with stable ids.
9. **Open paths lose their outer end handles.** A `BezPath` has nowhere to
   keep them; it matters once paths can be continued from an end.
10. **Colours are edited in 8-bit sRGB.** The picker and the wasm boundary
    speak sRGB bytes while the document keeps linear floats. Harmless while
    all colour is sRGB; a wide-gamut or HDR colour would be clipped to 8 bits
    the moment it is edited. The boundary would then carry floats, and the
    picker its own colour space.
11. **Text is shaped simply.** No ligatures, and scripts that need real
    shaping — Arabic, Indic, Thai — come out wrong. A full shaper
    (rustybuzz, harfrust) replaces `shape_line`; it costs 200–300 KB of
    gzipped wasm, so it may want loading only when such text appears.
12. **Text boxes only grow.** Lines break at newlines, never to a width;
    there is one weight, regular. Fixed-width boxes and bold need wrapping
    in the layout pass and more font slices.
13. **The text field's caret follows the browser's font metrics.** The
    core uses the font's ascender and descender; a browser on some systems
    uses the OS/2 metrics instead, which can put the caret a pixel or two
    off the glyphs. The glyphs themselves are always the core's.

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
