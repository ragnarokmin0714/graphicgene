# graphicgene

Web-based vector graphics editor (Illustrator-like), built so that raster
(Photoshop-like) and UI design (Figma-like) features can be added later without
rewriting the core.

## Status

Drawing, selection and transforms work: rectangles and ellipses are dragged
out on the artboard, and nodes can be selected (click, Shift-click, marquee,
layer panel), moved, scaled and rotated with handles, nudged and deleted — each
one undo step. The pen tool draws bezier paths, and any path (rectangles and
ellipses included) can be edited point by point. The project autosaves to
IndexedDB and is restored on the next visit; project files can be downloaded
and opened, and the artwork exports as SVG.

Every v0.1 feature is in. What stands between that and "done" is Roger's
browser pass over everything since 2026-09-25 (below) and the Pages deploy.

An architecture pass followed on 2026-09-27: the editing session moved from
the wasm crate into core, rendering became incremental with zero-copy pixels,
and the artboard size became document state. What is next, and the known
architectural debt, is in `ROADMAP.md`.

**Verified — the bar for any change:**

- `cargo test --workspace` — 81 tests, including a randomized check that
  incremental redraws equal full redraws pixel for pixel
- `cargo clippy --workspace --all-targets -- -D warnings`
- the web build (`tsc -b` + Vite)
- `pnpm smoke` — the real wasm module end to end, asserting on pixels
- `pnpm ui` — the React app driven in jsdom against the real core: 77 checks,
  including that the canvas equals a full redraw of the same document

Anything on a per-frame path also gets `pnpm bench` before and after; see
Performance rules.

**Browser-checked 2026-09-25** by Roger on the deployed Pages build: shapes
appear, undo/redo and their disabled states are right, save -> reload -> load
restores the document, edges are crisp, and the console is clean apart from a
missing favicon (since fixed). This box has no browser engine, so anything
changed after that date is verified headlessly only until he looks again — in
particular the theme switch, the redesigned chrome, and all canvas
interaction (tools, handles, marquee, pen, path editing, shortcuts), and
autosave / restore / file open / downloads. The React side of all that is
exercised by `pnpm ui` (jsdom, the real wasm core, fake-indexeddb), which is
the closest this box and CI get to a browser — but it cannot judge layout,
feel or looks.

## Commands

```
pnpm build:wasm   # wasm-pack build -> apps/web/src/wasm (gitignored)
pnpm dev          # build wasm, then Vite dev server
pnpm build        # production build of the web app
pnpm smoke        # wasm boundary end-to-end, no browser needed
pnpm ui           # the React app in jsdom against the real core
pnpm bench        # per-frame costs; compare before and after perf work
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

Rule: if a feature is not in the current milestone (see `ROADMAP.md`), no
crate, trait, or module exists for it yet.

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
- `RenderScene` is a flat snapshot. The renderer never walks the live
  document.
- **Only what changed is redrawn.** Every node mutation goes through
  `Document::get_mut`, `attach` or `detach`, which record it in a change log.
  `RenderScene::update` drains that log, refreshes just the affected items and
  reports `Damage` — where they were plus where they are — and the renderer
  redraws only that rect. Never add a way to change a node that bypasses those
  three methods: the canvas would silently stop updating.
- A partial redraw must equal a full redraw pixel for pixel;
  `graphicgene-render/tests/incremental.rs` checks it over random edits. The
  CPU renderer's scratch buffer is full-size on purpose — `cpu.rs` explains
  the seams a rect-sized one leaves.

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

### The editing session lives in core

`graphicgene_core::session::Session` owns the document, the undo journal, the
selection and whichever interaction is in progress, and every rule tying them
together: undo while drawing removes a pen anchor, Delete means whatever the
current mode has selected, Enter and Escape leave a mode, undo prunes the
selection. It takes typed arguments in document space.

A platform shell — `graphicgene-wasm` today, a Tauri app later — only
translates: ids to strings, pointer positions to document points, results to
JSON and pixels. **A rule written in a shell is a rule the two apps will
disagree on**, and one only JS can test. Pick tolerances look like shell
logic but are not: they are screen distances divided by the zoom, which only
the shell knows, so the shell measures them and passes them in — the rules
that use them stay in core.

The web shell still decides which core API a press goes to (`Stage.tsx`:
tool → pen, path edit or gesture). A native UI would need that routing too;
moving it into core is on the roadmap.

### IO boundary

**Core crates perform no IO.** They produce and consume bytes; the app layer
does storage. This is not a style preference: IndexedDB is async and `std::fs`
is sync, and a core that assumes either one cannot run on the other platform.

### State ownership

The document lives in Rust. React renders a view of it and sends commands back.
React must never hold authoritative document state — the moment it does, the web
and desktop apps drift apart and the core stops being the product.

That includes the artboard: its size is `Document::artboard`, saved in the
file, and the web app reads it from the core (`editor.width` / `height`).
The 800 × 600 in `App.tsx` only seeds a brand-new document.

What React does keep is view state — the active tool, the theme, status-bar
messages — plus caches of core data keyed on a version the core hands out
(`layersVersion`), never a copy it edits.

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
- The pen and path editing work on an anchor view of the path
  (`anchors.rs`); the file still stores plain `BezPath`s. A pen path joins
  the journal only when finished, so undo while drawing removes the last
  anchor (`pen.rs`); path edits commit one `SetPath` per drag
  (`path_edit.rs`). Anchor ids are positions, so after anything that adds or
  removes anchors, path editing drops its anchor selection rather than let it
  point at a different anchor.
- Undo/redo and replay are what the journal actually buys. **It does not decide
  collaboration**: multiplayer needs a conflict model (tree CRDT for node moves,
  or a server-authoritative sequencer), deferred until there is a reason to pick
  one.

### Project file

Versioned JSON, backward compatible from v0.1 onward. The `version` field is read
before anything else, and unknown fields round-trip rather than being dropped.

- serde_json's `float_roundtrip` feature is load-bearing: without it, parsing
  can be one ulp off, and since autosave re-reads the project on every visit,
  geometry would drift. `geometry_survives_save_and_load_bit_for_bit` guards
  it.
- The written copy drops detached nodes (`purge_unreachable` on a clone); the
  live document keeps them because undo needs them.
- On the web the project lives in IndexedDB (`storage.ts`). Autosave stays off
  until the stored project has been read back, and for the whole session if
  it could not be loaded, so an empty document never overwrites a project this
  build failed to open.

## Repo layout

Three crates. New crates appear when compile time or dependency isolation
demands them, not in advance.

The core modules in the order data flows: `doc` (arena + change log),
`command` (journal), `session` (the rules), then what the session drives —
`selection`, `hit`, `gesture`, `anchors`, `pen`, `path_edit` — and the
outputs: `layout`, `svg`, `project`.

```
graphicgene/
├── Cargo.toml              # Cargo workspace
├── pnpm-workspace.yaml     # pnpm workspace (pnpm only)
├── crates/
│   ├── graphicgene-core/   # document, commands, the editing session, SVG, project file
│   ├── graphicgene-render/ # incremental RenderScene, Renderer trait, CPU renderer
│   └── graphicgene-wasm/   # thin wasm-bindgen shell over the session
└── apps/
    └── web/
        ├── src/
        │   ├── components/ui/   # vendored shadcn — edit freely, that is the point
        │   ├── editor.ts        # the only file that touches wasm
        │   └── styles.css       # Tailwind theme + density tokens
        └── scripts/             # smoke.mjs (wasm boundary), ui.mjs (React in jsdom),
                                 # bench.mjs (per-frame costs)
```

`graphicgene-core` stays dependency-light (`kurbo`, `slotmap`, `serde`,
`thiserror`); heavy rendering dependencies stay in `graphicgene-render`.

`apps/desktop` (Tauri) and `packages/ui` are created when they are built, not
before. The crate prefix matches gamegene's `gamegene-*` convention — `gg-` is
ambiguous between the two projects.

## Performance rules

- No allocation in per-frame paths. The scene is updated in place from the
  change log and rebuilt only when the tree's shape changes. Known exceptions:
  each refreshed item clones its path, and the overlay crosses as JSON.
- Pixels never cross the wasm boundary by copy. `render()` returns the changed
  rect; the page reads the pixels in place from wasm memory
  (`EditorHandle.pixels()`) and blits only that rect. They are premultiplied
  RGBA, which equals the straight alpha `ImageData` expects only while every
  pixel is opaque — true while the artboard is painted white.
- Views cache core data on versions (`layersVersion`) that hold still through
  a drag, so a drag does not rebuild the layer panel every frame.
- No `Box<dyn ..>` in hot loops.
- The WASM boundary is a batching boundary: one call per interaction, never one
  call per node. Hot data crosses as a typed buffer, not per-node JSON.
- Hit-testing and damage will both need spatial acceleration eventually. Both
  still scan linearly — hit-testing with a control-box broad phase — but no
  interface may expose that assumption.
- Profile before tuning `opt-level`. Start WASM release at `opt-level = "s"` +
  `lto = true` + `wasm-opt`. `"z"` often costs real runtime in geometry-heavy
  code; the tradeoff must be measured, not assumed.

Frame costs from `pnpm bench` — 500 nodes on an 800 × 600 artboard, Node 24
on this box, mean per frame. Update this table whenever a per-frame path
changes; the 2026-09-27 column is before incremental rendering.

| Scenario | 2026-09-27 before | now |
|---|---|---|
| drag one shape (update + pixels to canvas) | 9.36 ms | 0.11 ms |
| drag everything (worst case: all damaged) | — | 9.2 ms |
| frame with nothing changed (selection, hover) | 9.31 ms | 0.001 ms |
| hover hit-test | 0.097 ms | 0.011 ms |
| read the layer rows | 0.62 ms, twice a frame | 0.68 ms, only when they change |

Current shipped size, so regressions are visible rather than gradual:

| Asset | Raw | Gzip |
|---|---|---|
| wasm (wasm-opt applied) | 769 KB | 299 KB |
| js (React + Radix + app) | 392 KB | 126 KB |
| css (incl. tw-animate-css) | 39 KB | 8 KB |
| font (Inter, latin subset) | 48 KB | — |

The browser fetches only the Inter subsets whose unicode-range the page uses,
so the other subset files in `dist/` cost nothing unless that script appears.

If the js figure climbs without a feature to show for it, check that
`lucide-react` and the `radix-ui` meta package are still tree-shaking.

## Roadmap

`ROADMAP.md` holds the milestones after v0.1, what each depends on, and the
known architectural debt. Keep it current: when a decision here changes what
a later step needs, say so there.

## Dependency policy

Track current stable versions rather than pinning to old ones — this project
has no backend, no user accounts and no data leaving the browser, so the
exposure from moving fast on versions is small and the cost of falling years
behind is not.

Two standing exceptions:

- **Verify, don't assume.** A major version goes in only after `cargo test`,
  `clippy --all-targets -D warnings`, the web build, `pnpm smoke` and
  `pnpm ui` all pass on it. "Latest" is a starting hypothesis, not a merge
  criterion.
- **Don't chase pre-1.0 crates that churn.** `wgpu` in particular breaks its API
  most releases; pin it when it arrives and upgrade deliberately.

Current floor: **Node >= 22.12** (Vite 8 requires it; see `.nvmrc`). Rust
**edition 2024**.

CI builds with the newest stable Rust, whatever this box has. A new release
can add clippy lints that fail CI with no code change — Rust 1.98's
`chunks_exact_to_as_chunks` did on 2026-09-27, while clippy here on 1.97 was
clean. Before trusting a local clippy run, `rustup check`; if stable has
moved, update it (or run `cargo +<version> clippy …` with CI's version).

Supply chain is the one real security surface here, so keep the dependency
count low — it is the reason core takes four crates and not fourteen.

The one deliberate exception is dev-only: `jsdom` and `fake-indexeddb` (about
30 packages) run `pnpm ui`, the only check of the React layer in CI. None of
it ships. Revisit if a lighter DOM ever covers what `ui.mjs` needs.

## Conventions

- Package manager: pnpm only (no npm/yarn).
- UI components come from shadcn and are vendored, not imported from a library.
  Editing them in place is expected — see The UI layer above.
- Rust errors: `thiserror` in library crates, `anyhow` in app/binary crates.
- Core crates must not reference browser or OS APIs. Platform differences go
  behind traits implemented at the app layer.
- `.ai` format is permanently out of scope.

## Deployment

- GitHub Actions: fmt, clippy, tests -> `wasm-pack build --release` -> smoke
  -> ui -> pnpm build -> GitHub Pages.
- Runners are pinned (`ubuntu-24.04`), not `ubuntu-latest`: a new image is a
  major version like any other, adopted by a commit once CI passes on it.
- Actions stay on majors that run on a current Node (24 as of 2026-09). When
  GitHub warns about a deprecated Node runtime, bump them — and replace any
  action that has stopped being maintained, as `jetli/wasm-pack-action` was
  (wasm-pack now comes from `cargo install --locked`).
- Use `Swatinem/rust-cache` in CI.
- The web app must respect the GitHub Pages base path (repo name).
- GitHub Pages cannot set COOP/COEP, so `SharedArrayBuffer` and WASM threads are
  unavailable there. If threads become necessary, move to Cloudflare Pages or
  Vercel — do not design v0.1 around threads.
