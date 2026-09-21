# graphicgene

A web-based vector graphics editor, built on a Rust core compiled to WebAssembly.

Designed so that raster and UI-design features can be added later without
rewriting the core — the document model, colour pipeline and render boundary are
already shaped for it.

> **Status: early.** The core, renderer and app shell work end to end — you can
> draw shapes, undo/redo, and save/reload a project. The pen tool, selection and
> SVG export are still missing, and nothing has been verified in a browser yet.

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
