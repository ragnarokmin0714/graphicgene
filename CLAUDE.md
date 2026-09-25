# graphicgene

Web-based vector graphics editor (Illustrator-like), built so that raster
(Photoshop-like) and UI design (Figma-like) features can be added later without
rewriting the core.

## Status

Drawing, selection and transforms work: rectangles and ellipses are dragged
out on the artboard, and nodes can be selected (click, Shift-click, marquee,
layer panel), moved, scaled and rotated with handles, nudged and deleted — each
one undo step. The pen tool and SVG export (plus IndexedDB autosave) are what
remain before v0.1 is done.

**Verified:** 33 Rust tests, `clippy --all-targets -D warnings` clean, the web
build, and `apps/web/scripts/smoke.mjs` — which drives the real wasm module
through draw / transform / undo / redo / save / reload and asserts on rendered
pixels, including a draw / select / drag / delete pass. These four are the
bar for any change.

**Browser-checked 2026-09-25** by Roger on the deployed Pages build: shapes
appear, undo/redo and their disabled states are right, save -> reload -> load
restores the document, edges are crisp, and the console is clean apart from a
missing favicon (since fixed). This box has no browser engine, so anything
changed after that date is verified headlessly only until he looks again — in
particular the theme switch, the redesigned chrome, and all canvas
interaction (tools, handles, marquee, shortcuts). The React side of that
interaction was exercised once in jsdom against the real wasm core — a
one-off scratch harness, not part of the repo — which is the closest this box
gets to a browser.

## Commands

```
pnpm build:wasm   # wasm-pack build -> apps/web/src/wasm (gitignored)
pnpm dev          # build wasm, then Vite dev server
pnpm build        # production build of the web app
pnpm smoke        # wasm boundary end-to-end, no browser needed
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings   # what CI runs
cargo fmt --all
```

`wasm-pack` is required for anything that touches the web app
(`cargo install wasm-pack`).

## Scope discipline

The long-term vision shapes **which decisions we make now**, not **how much we
build now**. Extensibility comes from getting a small number of hard-to-reverse
decisions right (see Load-bearing decisions), not from creating empty crates or
speculative plugin traits.

Rule: if a feature is not in v0.1, no crate, trait, or module exists for it yet.

## v0.1 — definition of done

A user can:

- draw rectangles and ellipses
- draw and edit bezier paths with a pen tool
- select, move, scale, rotate nodes; see them in a layer panel
- undo/redo every mutation
- save and reload a project (versioned JSON, IndexedDB autosave)
- export SVG

Explicitly NOT in v0.1: text, boolean ops, gradients, raster layers, components,
collaboration, desktop build, GPU rendering.

v0.1 ships to GitHub Pages. Nothing short of that counts as done.

## Load-bearing decisions

These are expensive to change later, and they are the entire reason the
Photoshop / Figma / desktop directions stay open.

### Document model

- Nodes live in an arena (`slotmap`), addressed by a stable `NodeId` — not
  `Rc<RefCell<..>>`. Arena storage is cache-friendly, serializable, and provides
  the stable identity that components and collaboration both depend on.
- `NodeKind` is an enum, not a trait object. `dyn` hurts both serialization and
  hot-path performance; adding `Raster` / `Component` / `Text` later is a new
  variant plus match arms the compiler will point us at.
- Geometry is `f64` in the document, `f32` at render time. A design tool loses
  precision visibly when zoomed if the model itself is f32.
- Every node carries `transform` (affine), `opacity`, `blend_mode`, and `clip`
  from day one, even while only `Normal` blending is implemented. These shape
  the render pipeline; retrofitting them means rewriting it.

### Color

- Colors are stored as `f32` RGBA in **linear sRGB**, never `u8` sRGB.
- This is the single decision that keeps Photoshop-class work reachable: 16-bit
  depth, wide gamut, and correct blending all require it, and all three become
  unreachable once 8-bit sRGB is baked into the model.

### Pipeline

Editing and drawing are separate stages, always:

```
document (arena) -> layout pass -> RenderScene (immutable snapshot) -> Renderer
```

- The **layout pass** is a no-op in v0.1. It exists so Figma-style auto layout
  has a place to live that is already wired into the pipeline.
- `RenderScene` is a flat immutable snapshot. The renderer never walks the live
  document.
- `Renderer` accepts a dirty rect. Redrawing everything is the v0.1
  *implementation*, not the v0.1 *interface*.

### Rendering

- The v0.1 renderer is **CPU** (`tiny-skia`), not `wgpu`/WebGPU:
  - the hard part of vector rendering is tessellation and antialiasing, not
    reaching the GPU
  - `tiny-skia` is the rasterizer behind `resvg`, which PNG export needs anyway
  - WebGPU device coverage is still uneven
- A GPU renderer (`wgpu`, or `vello`) is the *second* implementation of the same
  trait. An abstraction with one implementation is not an abstraction — the CPU
  renderer is what proves the trait is honest before desktop work begins.

### Geometry

Use `kurbo` for bezier math, and `lyon` when GPU tessellation arrives.
Hand-rolled bezier and boolean code is a multi-month detour with worse numerics.

### IO boundary

**Core crates perform no IO.** They produce and consume bytes; the app layer
does storage. This is not a style preference: IndexedDB is async and `std::fs`
is sync, and a core that assumes either one cannot run on the other platform.

### State ownership

The document lives in Rust. React renders a view of it and sends commands back.
React must never hold authoritative document state — the moment it does, the web
and desktop apps drift apart and the core stops being the product.

### The UI layer: Tailwind v4 + shadcn

Chrome is built with Tailwind v4 and shadcn components vendored under
`apps/web/src/components/ui/`. The reason is Radix underneath shadcn: focus
traps, roving tabindex, dismiss behaviour and keyboard navigation for menus and
popovers are easy to get subtly wrong by hand and tedious to debug. It is the
same argument that chose React over an immediate-mode toolkit, one level down.

Two rules keep it from spreading where it does not belong:

- **Density is set in the tokens and in the vendored source, not per component.**
  shadcn targets dashboards (36px controls); editor chrome is Figma scale. The
  button's `defaultVariants` is changed to `xs` (24px) in the vendored file, and
  `--spacing-row` / `--text-ui` / `--text-label` in `styles.css` carry the rest.
  Tune density there — never by sprinkling one-off heights, which is how twenty
  panels end up disagreeing.
- **Tailwind styles chrome only.** Selection handles, snap guides, rulers and
  anything else positioned from computed geometry use inline styles or CSS
  transforms: those numbers come from the core, not from a design token. The
  canvas element's own `width`/`height` are device pixels for the same reason.

Theme is a per-viewer preference, not document state: `useTheme.ts` keeps it
in React and localStorage, and an inline script in `index.html` applies it
before first paint. Keep the two in sync. Keyboard shortcuts go through
`useShortcuts` in `shortcuts.ts`, which already skips text fields — do not add
ad-hoc `keydown` listeners. The one exception is `Stage.tsx` tracking Shift
and Alt during a drag, which re-applies a constraint rather than running a
command.

Selection handles are the one place geometry is split: the core reports the
selection frame's corners (`overlay()`), and `handles.ts` places handles and
hit-tests them, because handle size and grab distance are screen
measurements that must not scale with zoom.

`--canvas-backdrop` is deliberately not `--background`: artwork has to be judged
against a neutral field, not against the UI's tint.

### Why React rather than an all-Rust UI

A design tool is mostly UI chrome: panels, menus, property inspectors,
drag-reorderable layer trees, colour pickers, dialogs, context menus, shortcut
handling. The canvas is one element among them. Three things decided it:

- **IME.** The text tool must accept Chinese input. A DOM input inherits the
  browser's input-method integration for free; immediate-mode Rust UI toolkits
  have long-standing gaps there. This alone is close to decisive.
- **Bundle size and first paint.** An all-Rust UI ships every widget as WASM,
  and the primary distribution channel is GitHub Pages.
- **Iteration speed.** UI is what changes most often; Vite HMR against a wasm
  rebuild cycle is not a close comparison.

The cost is a real boundary to maintain, which is why React's role is kept as
small as it is (see State ownership above) — it is a replaceable view layer, not
the application. If this is ever revisited, the credible alternative is a
DOM-rendering Rust framework such as Dioxus, not an immediate-mode toolkit.

### Mutations

- Every document change is a `Command` with `apply` / `undo`, appended to a
  journal.
- Commands are serializable, which is what keeps a networked design possible.
- Drags (move, scale, rotate, drawing a shape) preview by writing straight
  into the document and commit as one `Command::Batch` on release, so a drag
  is one undo step and Escape restores the press-time state. See
  `gesture.rs`. Selection is session state in core (`selection.rs`): not
  saved, not undoable, but shared with the future desktop app.
- Undo/redo and replay are what the journal actually buys. **It does not decide
  collaboration**: multiplayer needs a conflict model (tree CRDT for node moves,
  or a server-authoritative sequencer), deferred until there is a reason to pick
  one.

### Project file

Versioned JSON, backward compatible from v0.1 onward. The `version` field is read
before anything else, and unknown fields round-trip rather than being dropped.

## Repo layout

Three crates. New crates appear when compile time or dependency isolation
demands them, not in advance.

```
graphicgene/
├── Cargo.toml              # Cargo workspace
├── pnpm-workspace.yaml     # pnpm workspace (pnpm only)
├── crates/
│   ├── graphicgene-core/   # nodes, commands, selection, hit-testing, gestures, layout, project file
│   ├── graphicgene-render/ # RenderScene, Renderer trait, CPU renderer
│   └── graphicgene-wasm/   # wasm-bindgen bindings (the batching boundary)
└── apps/
    └── web/
        ├── src/
        │   ├── components/ui/   # vendored shadcn — edit freely, that is the point
        │   ├── editor.ts        # the only file that touches wasm
        │   └── styles.css       # Tailwind theme + density tokens
        └── scripts/             # browser-free smoke test of the wasm boundary
```

`graphicgene-core` stays dependency-light (`kurbo`, `slotmap`, `serde`,
`thiserror`); heavy rendering dependencies stay in `graphicgene-render`.

`apps/desktop` (Tauri) and `packages/ui` are created when they are built, not
before. The crate prefix matches gamegene's `gamegene-*` convention — `gg-` is
ambiguous between the two projects.

## Performance rules

- No allocation in per-frame paths. `RenderScene` is rebuilt per edit, not per
  frame.
- No `Box<dyn ..>` in hot loops.
- The WASM boundary is a batching boundary: one call per interaction, never one
  call per node. Hot data crosses as a typed buffer, not per-node JSON.
- Hit-testing and rendering will both need spatial acceleration eventually. v0.1
  does linear scans, but no interface may expose that assumption.
- Profile before tuning `opt-level`. Start WASM release at `opt-level = "s"` +
  `lto = true` + `wasm-opt`. `"z"` often costs real runtime in geometry-heavy
  code; the tradeoff must be measured, not assumed.

Current shipped size, so regressions are visible rather than gradual:

| Asset | Raw | Gzip |
|---|---|---|
| wasm (wasm-opt applied) | 675 KB | 251 KB |
| js (React + Radix + app) | 381 KB | 123 KB |
| css (incl. tw-animate-css) | 39 KB | 8 KB |
| font (Inter, latin subset) | 48 KB | — |

The browser fetches only the Inter subsets whose unicode-range the page uses,
so the other subset files in `dist/` cost nothing unless that script appears.

If the js figure climbs without a feature to show for it, check that
`lucide-react` and the `radix-ui` meta package are still tree-shaking.

## After v0.1 — and what each step depends on

| Step | What it needs |
|---|---|
| Text (v0.2) | `cosmic-text` / `rustybuzz` shaping, font loading, text->path on export. Large enough to be its own milestone; deliberately out of v0.1. |
| Boolean ops | `kurbo` path intersection. Numerically the hardest vector feature. |
| Desktop (Tauri) | Nothing new in core — if the IO-boundary and state-ownership rules held. This step is the test of whether they did. |
| GPU renderer | A second `Renderer` impl (`wgpu` / `vello`). |
| Raster (Photoshop-ish) | New `NodeKind`, pixel buffers, real blend modes. Possible only because of the linear-`f32` color decision. |
| Components / auto layout (Figma-ish) | Node references plus overrides on stable `NodeId`s; auto layout fills in the existing layout pass. |
| Collaboration | A backend, and a conflict model chosen at that time. |

## Dependency policy

Track current stable versions rather than pinning to old ones — this project
has no backend, no user accounts and no data leaving the browser, so the
exposure from moving fast on versions is small and the cost of falling years
behind is not.

Two standing exceptions:

- **Verify, don't assume.** A major version goes in only after `cargo test`,
  `clippy --all-targets -D warnings`, the web build and `pnpm smoke` all pass on
  it. "Latest" is a starting hypothesis, not a merge criterion.
- **Don't chase pre-1.0 crates that churn.** `wgpu` in particular breaks its API
  most releases; pin it when it arrives and upgrade deliberately.

Current floor: **Node >= 22.12** (Vite 8 requires it; see `.nvmrc`). Rust
**edition 2024**.

Supply chain is the one real security surface here, so keep the dependency
count low — it is the reason core takes four crates and not fourteen.

## Conventions

- Package manager: pnpm only (no npm/yarn).
- UI components come from shadcn and are vendored, not imported from a library.
  Editing them in place is expected — see The UI layer above.
- Rust errors: `thiserror` in library crates, `anyhow` in app/binary crates.
- Core crates must not reference browser or OS APIs. Platform differences go
  behind traits implemented at the app layer.
- `.ai` format is permanently out of scope.

## Deployment

- GitHub Actions: `wasm-pack build --release` -> pnpm build -> GitHub Pages.
- Use `Swatinem/rust-cache` in CI.
- The web app must respect the GitHub Pages base path (repo name).
- GitHub Pages cannot set COOP/COEP, so `SharedArrayBuffer` and WASM threads are
  unavailable there. If threads become necessary, move to Cloudflare Pages or
  Vercel — do not design v0.1 around threads.
