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

## v0.4 — desktop · step 1 done, `v0.4.0`

The test of the architecture, in two steps:

1. ~~**The same web app in a Tauri webview.**~~ **Built 2026-10-05.** Only
   storage changed: the autosave is `autosave.json` in the app's data
   folder, and opening, saving and exporting go through the system's
   dialogs. The rule held — the core did not change; `platform.ts` picks the
   IO, and the desktop half is a chunk the web never fetches. The page's
   file access is four commands of our own (`apps/desktop/src-tauri/src/files.rs`),
   not Tauri's fs plugin, so it writes only where the user picked. The
   Desktop workflow builds Windows, macOS and Linux installers, and drafts
   a release with them on a version tag — from `v0.4.1` on: `v0.4.0`'s
   builds passed on all three systems, but its release step attached the
   bundle folders instead of the files in them. Owed: a look at the real window on
   Windows — this box has no display, and its webview's CSP (wasm, inline
   styles) is checked only by reading it.
2. **The core running natively**, behind Tauri commands, once threads or a
   GPU renderer are worth it. The session API is already shell-agnostic;
   this is a new shell, not a new core. Not started: nothing needs threads
   or a GPU yet.

## v0.5 — editing basics · in progress

Chosen by Roger on 2026-10-06, after v0.4: what anyone reaches for once
shapes are on the page — like v0.2, many small things rather than one big
one.

| Item | What it needs |
|---|---|
| ~~Align and distribute~~ | **Done 2026-10-06.** `core::align`: line the selection up by an edge or centre — several layers with each other, one with the artboard — or even out the gaps between three or more. Each layer moves by its box on the page, composed through its parent like a drag, and a group moves as one with anything selected inside it. One batch, one undo step. Buttons at the top of the properties panel. |
| ~~Stroke styles~~ | **Done 2026-10-07.** Caps (flat, round, square), joins (sharp, round, bevelled) and dashes (a length and a gap) on `Stroke`, set in the stroke section of the panel. tiny-skia draws them and SVG writes them, each only when not the default, so plain strokes are written as before. The project format is version 3: an older build refuses a file rather than drawing its dashes solid and dropping them on the next save. The renderer's damage reach already allowed for the longest miter (four half-widths), and the randomized redraw test now draws sharp triangles and open zigzags with random caps, joins and dashes to hold it to that. |
| ~~Gradient fills~~ | **Done 2026-10-08.** A fill is a `Paint`: one colour, or a linear or radial gradient of two or more stops, kept in the unit square of the shape's own box so it stretches and turns with the shape — SVG's `objectBoundingBox`, which is how export writes it. A solid fill is written bare, as fills always were, so older files read unchanged. The panel switches a fill between solid, linear and radial (a colour fades out into a gradient; a gradient keeps its first colour when made solid), edits each stop's colour and position, adds a stop in the widest gap in the colour already there (mixed in sRGB, as it is drawn), removes stops down to two, and turns a linear gradient. Owed: handles on the canvas to drag a gradient's ends. |
| Snapping and smart guides | While moving and drawing, edges and centres snap to other layers' and the artboard's, with guide lines drawn over the canvas. Per-frame: candidates gathered when the drag starts, no allocation while it runs; `pnpm bench` before and after. |

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
6. **Per-frame leftovers.** Each refreshed item clones its path (and a
   gradient's stops), the overlay crosses the boundary as JSON, and every
   draw converts its path to tiny-skia's type — and a dashed stroke's
   pattern or a gradient's stops, which tiny-skia takes as `Vec`s. None shows in `pnpm bench` today; measure before
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
