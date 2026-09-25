# graphicgene

A web-based vector graphics editor, built on a Rust core compiled to WebAssembly.

Designed so that raster and UI-design features can be added later without
rewriting the core — the document model, colour pipeline and render boundary are
already shaped for it.

> **Status: v0.1 preview.** Draw rectangles, ellipses and bezier paths with a
> pen tool; select, move, scale and rotate them; edit any path point by point;
> undo everything. Work autosaves in the browser, project files can be
> downloaded and reopened, and the artwork exports as SVG.

### Using it

| | |
|---|---|
| `V` `R` `O` `P` | Select, Rectangle, Ellipse, Pen |
| Drag with a shape tool | Draw it — `Shift` for equal sides, `Alt` from the centre |
| Pen | Click for a corner, drag for a curve; click the first point to close, `Enter` to finish |
| Double-click a path | Edit its points: drag points and handles, click a segment to add a point, double-click a point for corner/curve |
| Handles on a selection | Scale (`Shift` keeps proportions, `Alt` from the centre); just outside a corner rotates (`Shift` snaps to 15°) |
| Arrow keys | Nudge 1px, 10px with `Shift` |
| `Ctrl/⌘ Z`, `Ctrl/⌘ Shift Z` | Undo, redo |
| `Ctrl/⌘ O`, `Ctrl/⌘ Shift E` | Open a project file, export SVG |

## Architecture

```
document (arena)  →  layout pass  →  RenderScene  →  Renderer
```

| Crate | Responsibility |
|---|---|
| `graphicgene-core` | Node tree, commands and undo, geometry, layout, project file. Performs no IO and touches no platform API. |
| `graphicgene-render` | Immutable render scene, the `Renderer` trait, and a CPU backend (`tiny-skia`). |
| `graphicgene-wasm` | The `wasm-bindgen` boundary — one call per interaction, never one per node. |

The UI (`apps/web`) is React + Tailwind + shadcn, and holds **no** document
state: the document lives in Rust, and React renders a view of it.

`CLAUDE.md` documents the decisions behind this — which ones are load-bearing,
and why — and is worth reading before changing anything structural.

## Getting started

Requires Rust, [`wasm-pack`](https://rustwasm.github.io/wasm-pack/), Node 24+
and pnpm.

```sh
pnpm install
pnpm dev      # builds the wasm package, then starts Vite
```

Other tasks:

```sh
pnpm build    # production build
pnpm smoke    # drives the real wasm module and asserts on rendered pixels
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Licence

Dual-licensed under MIT or Apache-2.0, at your option.
