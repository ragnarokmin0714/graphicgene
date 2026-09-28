//! wasm-bindgen bindings: a thin shell over `graphicgene_core::Session`.
//!
//! This is the batching boundary. One call per user interaction, never one
//! call per node — a chatty WASM boundary is the most common way a project
//! like this ends up slow, and it is very hard to undo once the UI depends on
//! the fine-grained shape.
//!
//! Nothing here decides how editing behaves; that is all in the session, so
//! a desktop shell gets the same rules for free. What lives here is what is
//! specific to this shell: node ids as strings, colours as sRGB bytes,
//! results as JSON, the view the canvas shows, and the pixels it is painted
//! from.
//!
//! **The page speaks screen pixels.** Every pointer position and pick
//! tolerance arrives in CSS pixels from the viewport's corner and is mapped
//! into the document here, through the view; the overlay goes back out in
//! screen pixels too. The page never converts coordinates itself.
//!
//! **Pixels never cross the boundary by copy.** `render` redraws only what
//! changed — shifting what it already has when the view only panned — and
//! says where; the page reads those parts in place from wasm memory through
//! `pixelsPtr`.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::command::Command;
use graphicgene_core::geom::{Affine, BezPath, Ellipse, Point, Rect, Shape, Size, Vec2};
use graphicgene_core::gesture::{Modifiers, ShapeKind, TransformKind};
use graphicgene_core::layers::{Arrange, Drop};
use graphicgene_core::node::{Node, NodeId, Stroke};
use graphicgene_core::path_edit::PressOutcome;
use graphicgene_core::properties::{Properties, Property, Shared};
use graphicgene_core::session::{Mode, Overlay, SelectOutcome, Session};
use graphicgene_core::view::{MAX_ZOOM, View};
use graphicgene_render::{
    CpuRenderer, Damage, PixelRect, RenderScene, Renderer, device_area, scroll,
};
use serde_json::{Value, json};
use slotmap::{Key, KeyData};
use tiny_skia::Pixmap;
use wasm_bindgen::prelude::*;

/// Stroke width for paths drawn with the pen, in document units.
const PEN_STROKE_WIDTH: f64 = 2.0;
/// Room left around the artboard when fitting it into the viewport, in
/// screen pixels.
const FIT_PADDING: f64 = 48.0;
/// The backdrop until the page sets its theme's: the light one.
const DEFAULT_BACKDROP: [u8; 3] = [235, 235, 235];

/// Everything the UI talks to.
#[wasm_bindgen]
pub struct Editor {
    session: Session,
    renderer: CpuRenderer,
    /// The viewport's pixels, in device pixels.
    pixmap: Pixmap,
    scene: RenderScene,
    view: View,
    /// The view the pixels were drawn for; `None` when they are not valid.
    shown: Option<View>,
    /// Whether every pixel is what a full redraw would give. Shifting pixels
    /// for a pan leaves edges the old canvas edge cut a hair off, until
    /// `settle` has them redrawn.
    exact: bool,
    /// Whether the page has sized the viewport. Until it does, the viewport
    /// is the artboard at 100% — which is how tests and headless uses see a
    /// document.
    shell_viewport: bool,
    /// Fit the artboard into the viewport at the next chance: a new or a
    /// freshly loaded document.
    fit_pending: bool,
}

#[wasm_bindgen]
impl Editor {
    /// A new document with a `width` × `height` artboard. A document loaded
    /// later brings its own size.
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32) -> Result<Editor, JsError> {
        let session = Session::with_artboard(Size::new(width.into(), height.into()));
        let view = View::new(width, height, 1.0);
        let mut scene = RenderScene::default();
        let [r, g, b] = DEFAULT_BACKDROP;
        scene.background = Some(LinearRgba::from_srgb8(r, g, b, 255));
        Ok(Editor {
            pixmap: pixmap_for(&view)?,
            session,
            renderer: CpuRenderer::new(),
            scene,
            view,
            shown: None,
            exact: true,
            shell_viewport: false,
            fit_pending: true,
        })
    }

    /// The canvas's width in device pixels: its backing store must match.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// The artboard's size in document units.
    #[wasm_bindgen(getter, js_name = artboardWidth)]
    pub fn artboard_width(&self) -> f64 {
        self.session.document().artboard().width
    }

    #[wasm_bindgen(getter, js_name = artboardHeight)]
    pub fn artboard_height(&self) -> f64 {
        self.session.document().artboard().height
    }

    // ---- The view ------------------------------------------------------------------

    /// The viewport's size in device pixels, and the device pixel ratio. The
    /// first call also fits the artboard into it.
    #[wasm_bindgen(js_name = setViewport)]
    pub fn set_viewport(&mut self, width: u32, height: u32, dpr: f64) -> Result<(), JsError> {
        self.shell_viewport = true;
        self.view.resize(width, height, dpr);
        if self.fit_pending {
            self.fit(1.0);
        }
        self.sync_pixmap()
    }

    /// Screen pixels per document unit.
    #[wasm_bindgen(getter)]
    pub fn zoom(&self) -> f64 {
        self.view.zoom()
    }

    /// Where the document origin is on screen, in CSS pixels.
    #[wasm_bindgen(getter, js_name = panX)]
    pub fn pan_x(&self) -> f64 {
        self.view.pan().x
    }

    #[wasm_bindgen(getter, js_name = panY)]
    pub fn pan_y(&self) -> f64 {
        self.view.pan().y
    }

    #[wasm_bindgen(getter)]
    pub fn dpr(&self) -> f64 {
        self.view.dpr()
    }

    /// Move the picture by a screen-space offset.
    #[wasm_bindgen(js_name = panBy)]
    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        self.view.pan_by(Vec2::new(dx, dy));
    }

    /// Multiply the zoom by `factor`, keeping the screen point (x, y) still.
    #[wasm_bindgen(js_name = zoomBy)]
    pub fn zoom_by(&mut self, factor: f64, x: f64, y: f64) {
        self.view.zoom_by(factor, Point::new(x, y));
    }

    /// The next power-of-two zoom up, around the viewport's centre.
    #[wasm_bindgen(js_name = zoomIn)]
    pub fn zoom_in(&mut self) {
        let centre = self.centre();
        self.view.zoom_in(centre);
    }

    #[wasm_bindgen(js_name = zoomOut)]
    pub fn zoom_out(&mut self) {
        let centre = self.centre();
        self.view.zoom_out(centre);
    }

    /// Set the zoom, around the viewport's centre.
    #[wasm_bindgen(js_name = zoomTo)]
    pub fn zoom_to(&mut self, zoom: f64) {
        let centre = self.centre();
        self.view.zoom_to(zoom, centre);
    }

    /// Show the whole artboard, as large as it fits.
    #[wasm_bindgen(js_name = zoomToFit)]
    pub fn zoom_to_fit(&mut self) {
        self.fit(MAX_ZOOM);
    }

    /// Set zoom and pan exactly, as when restoring a view.
    #[wasm_bindgen(js_name = setView)]
    pub fn set_view(&mut self, zoom: f64, pan_x: f64, pan_y: f64) {
        self.view.set(zoom, Vec2::new(pan_x, pan_y));
    }

    /// The colour around the artboard, as sRGB bytes — the theme's backdrop.
    #[wasm_bindgen(js_name = setBackdrop)]
    pub fn set_backdrop(&mut self, r: u8, g: u8, b: u8) {
        let colour = Some(LinearRgba::from_srgb8(r, g, b, 255));
        if self.scene.background != colour {
            self.scene.background = colour;
            self.shown = None;
        }
    }

    // ---- Document and history ------------------------------------------------

    /// Add a rectangle. Returns the new node's id as a string the UI can hold.
    #[wasm_bindgen(js_name = addRect)]
    pub fn add_rect(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        srgb: &[u8],
    ) -> Result<String, JsError> {
        let path = Rect::new(x, y, x + width, y + height).to_path(0.1);
        self.insert(Node::vector("Rectangle", path, Some(colour(srgb)?)))
    }

    #[wasm_bindgen(js_name = addEllipse)]
    pub fn add_ellipse(
        &mut self,
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        srgb: &[u8],
    ) -> Result<String, JsError> {
        let path = Ellipse::new((cx, cy), (rx, ry), 0.0).to_path(0.1);
        self.insert(Node::vector("Ellipse", path, Some(colour(srgb)?)))
    }

    /// Add a path from an SVG path string.
    #[wasm_bindgen(js_name = addPath)]
    pub fn add_path(&mut self, svg_path: &str, srgb: &[u8]) -> Result<String, JsError> {
        let path = BezPath::from_svg(svg_path).map_err(|e| JsError::new(&e.to_string()))?;
        self.insert(Node::vector("Path", path, Some(colour(srgb)?)))
    }

    #[wasm_bindgen(js_name = setTransform)]
    pub fn set_transform(&mut self, id: &str, m: &[f64]) -> Result<(), JsError> {
        let &[a, b, c, d, e, f] = m else {
            return Err(JsError::new("transform needs 6 coefficients"));
        };
        let id = self.id(id)?;
        let transform = Affine::new([a, b, c, d, e, f]);
        self.session
            .execute(Command::SetTransform { id, transform })
            .map_err(to_js)
    }

    pub fn undo(&mut self) -> Result<bool, JsError> {
        self.session.undo().map_err(to_js)
    }

    pub fn redo(&mut self) -> Result<bool, JsError> {
        self.session.redo().map_err(to_js)
    }

    #[wasm_bindgen(js_name = canUndo)]
    pub fn can_undo(&self) -> bool {
        self.session.can_undo()
    }

    #[wasm_bindgen(js_name = canRedo)]
    pub fn can_redo(&self) -> bool {
        self.session.can_redo()
    }

    /// Whether a press is in progress, so the document holds a preview that
    /// should not be saved yet.
    pub fn busy(&self) -> bool {
        self.session.busy()
    }

    /// The project file. Core does no IO — the caller decides where these
    /// bytes go (IndexedDB on web, `std::fs` on desktop).
    #[wasm_bindgen(js_name = toJson)]
    pub fn to_json(&self) -> Result<String, JsError> {
        self.session.save().map_err(to_js)
    }

    /// Replace the document and fit its artboard into the viewport. Read
    /// `width`, `height` and the artboard size again afterwards.
    #[wasm_bindgen(js_name = loadJson)]
    pub fn load_json(&mut self, text: &str) -> Result<(), JsError> {
        self.session.load(text).map_err(to_js)?;
        self.fit_pending = true;
        if self.shell_viewport {
            self.fit(1.0);
        } else {
            let artboard = self.session.document().artboard();
            self.view = View::new(
                artboard.width.ceil() as u32,
                artboard.height.ceil() as u32,
                1.0,
            );
        }
        self.sync_pixmap()
    }

    /// The artwork as SVG, the size of the artboard.
    #[wasm_bindgen(js_name = exportSvg)]
    pub fn export_svg(&self) -> Result<String, JsError> {
        self.session.export_svg().map_err(to_js)
    }

    // ---- Pixels ------------------------------------------------------------------

    /// Bring the pixels up to date. Returns, as a flat list of device pixels:
    /// `[dx, dy, n, x, y, width, height, …]` — first shift the canvas by
    /// (dx, dy), then put back the `n` rects that follow. `[0, 0, 0]` means
    /// nothing changed, as after a selection click or a hover.
    ///
    /// Read the pixels in place with `pixelsPtr`.
    pub fn render(&mut self) -> Result<Vec<i32>, JsError> {
        let changes = self.session.prepare_render().map_err(to_js)?;
        let damage = self
            .scene
            .update(self.session.document(), &changes)
            .map_err(to_js)?;
        let (width, height) = (self.pixmap.width(), self.pixmap.height());
        let device = self.view.to_device();

        let mut everything = damage == Damage::Everything;
        let mut shift = (0, 0);
        let mut dirty = Vec::new();
        match self.shown.map(|shown| self.view.scroll_from(&shown)) {
            None | Some(None) => everything = true,
            Some(Some((0, 0))) => {}
            Some(Some((dx, dy))) if dx.unsigned_abs() >= width || dy.unsigned_abs() >= height => {
                everything = true;
            }
            Some(Some((dx, dy))) => {
                let strips = scroll(&mut self.pixmap, dx, dy);
                dirty.extend(strips.into_iter().map(PixelRect::bounds));
                shift = (dx, dy);
                self.exact = false;
            }
        }
        if let Damage::Region(region) = damage {
            dirty.push(device_area(device, region));
        }
        if everything {
            dirty = vec![Rect::new(0.0, 0.0, width.into(), height.into())];
            shift = (0, 0);
            self.exact = true;
        }

        let mut out = vec![shift.0, shift.1, 0];
        for area in dirty {
            let Some(rect) = PixelRect::covering(area, width, height) else {
                continue;
            };
            self.renderer
                .render(&self.scene, device, rect.bounds(), &mut self.pixmap)
                .map_err(|e| JsError::new(&e.to_string()))?;
            out.extend([rect.x, rect.y, rect.width, rect.height].map(|v| v as i32));
        }
        out[2] = ((out.len() - 3) / 4) as i32;
        self.shown = Some(self.view);
        Ok(out)
    }

    /// Call once a pan has come to rest. Returns whether the pixels need a
    /// full redraw to be exact again; if so, the next `render` does it.
    pub fn settle(&mut self) -> bool {
        if self.exact {
            return false;
        }
        self.shown = None;
        true
    }

    /// Where the canvas pixels start in wasm memory: `width * height * 4`
    /// bytes of RGBA, premultiplied — and opaque, since the backdrop is. The
    /// same address until the viewport changes size; the page must still
    /// rebuild its view whenever wasm memory grows, which detaches the old
    /// buffer.
    #[wasm_bindgen(js_name = pixelsPtr)]
    pub fn pixels_ptr(&self) -> *const u8 {
        self.pixmap.data().as_ptr()
    }

    /// A copy of the canvas pixels. For tests and exports, not for frames.
    #[wasm_bindgen(js_name = pixelBytes)]
    pub fn pixel_bytes(&self) -> Vec<u8> {
        self.pixmap.data().to_vec()
    }

    // ---- Layer panel -------------------------------------------------------------

    /// Changes whenever the layer rows may have: cache `layerTree` on it.
    #[wasm_bindgen(js_name = layersVersion)]
    pub fn layers_version(&self) -> String {
        self.session.layers_version().to_string()
    }

    /// The layer rows, as JSON, in panel order (topmost first, no root).
    #[wasm_bindgen(js_name = layerTree)]
    pub fn layer_tree(&self) -> Result<String, JsError> {
        let rows: Vec<Value> = self
            .session
            .layer_rows()
            .map_err(to_js)?
            .into_iter()
            .map(|row| {
                json!({
                    "id": encode_id(row.id),
                    "depth": row.depth,
                    "selected": row.selected,
                    "name": row.name,
                    "visible": row.visible,
                    "locked": row.locked,
                    "opacity": row.opacity,
                    "kind": if row.is_group { "group" } else { "vector" },
                })
            })
            .collect();
        to_json(&Value::Array(rows))
    }

    /// Rename a layer; false for a blank name or the one it has.
    pub fn rename(&mut self, id: &str, name: &str) -> Result<bool, JsError> {
        let id = self.id(id)?;
        self.session.rename(id, name).map_err(to_js)
    }

    /// Show or hide one layer: the eye on its row.
    #[wasm_bindgen(js_name = setVisible)]
    pub fn set_visible(&mut self, id: &str, visible: bool) -> Result<bool, JsError> {
        let id = self.id(id)?;
        self.session.set_visible(&[id], visible).map_err(to_js)
    }

    /// Lock or unlock one layer: the lock on its row.
    #[wasm_bindgen(js_name = setLocked)]
    pub fn set_locked(&mut self, id: &str, locked: bool) -> Result<bool, JsError> {
        let id = self.id(id)?;
        self.session.set_locked(&[id], locked).map_err(to_js)
    }

    /// Hide the selection, or show it if all of it is hidden.
    #[wasm_bindgen(js_name = toggleVisible)]
    pub fn toggle_visible(&mut self) -> Result<bool, JsError> {
        self.session.toggle_visible().map_err(to_js)
    }

    /// Lock the selection, or unlock it if all of it is locked.
    #[wasm_bindgen(js_name = toggleLocked)]
    pub fn toggle_locked(&mut self) -> Result<bool, JsError> {
        self.session.toggle_locked().map_err(to_js)
    }

    /// Move the selected layers where they were dropped: `place` is
    /// "above" or "below" the row `id`, or "inside" it, a group.
    #[wasm_bindgen(js_name = moveSelection)]
    pub fn move_selection(&mut self, id: &str, place: &str) -> Result<bool, JsError> {
        let id = self.id(id)?;
        let drop = match place {
            "above" => Drop::Above(id),
            "below" => Drop::Below(id),
            "inside" => Drop::Inside(id),
            _ => return Err(JsError::new("place is above, below or inside")),
        };
        self.session.move_selection(drop).map_err(to_js)
    }

    /// "forward", "backward", "front" or "back".
    pub fn arrange(&mut self, how: &str) -> Result<bool, JsError> {
        let arrange = match how {
            "forward" => Arrange::Forward,
            "backward" => Arrange::Backward,
            "front" => Arrange::ToFront,
            "back" => Arrange::ToBack,
            _ => return Err(JsError::new("arrange forward, backward, front or back")),
        };
        self.session.arrange(arrange).map_err(to_js)
    }

    /// Put the selection in a new group, which becomes the selection.
    pub fn group(&mut self) -> Result<bool, JsError> {
        self.session.group_selection().map_err(to_js)
    }

    /// Dissolve the selected groups into their parents.
    pub fn ungroup(&mut self) -> Result<bool, JsError> {
        self.session.ungroup_selection().map_err(to_js)
    }

    // ---- Clipboard -------------------------------------------------------------------
    //
    // Text in and out: the page moves it through the system clipboard.

    /// The selection as clipboard text; undefined with nothing selected.
    pub fn copy(&self) -> Result<Option<String>, JsError> {
        self.session.copy_selection().map_err(to_js)
    }

    /// Copy the selection, then delete it as one undo step.
    pub fn cut(&mut self) -> Result<Option<String>, JsError> {
        self.session.cut_selection().map_err(to_js)
    }

    /// Paste clipboard text where it was copied from; false for text that
    /// is not graphicgene nodes.
    pub fn paste(&mut self, text: &str) -> Result<bool, JsError> {
        self.session.paste(text).map_err(to_js)
    }

    /// Copy the selection in place, each copy right above its original.
    pub fn duplicate(&mut self) -> Result<bool, JsError> {
        self.session.duplicate_selection().map_err(to_js)
    }

    // ---- Properties panel ----------------------------------------------------------
    //
    // Values here are what the user types — document units, degrees — not
    // pointer positions, so they bypass the view. Colours are 4 sRGB bytes.

    /// What the properties panel shows, as JSON: `null` with nothing
    /// selected, else `{count, x, y, width, height, rotation, opacity, fill,
    /// stroke, strokeWidth}`. Rotation is in degrees, counter-clockwise. A
    /// value the selected nodes do not share is `"mixed"`. A colour is
    /// `[r, g, b, a]`, or `null` for none; `fill` and `stroke` are left out
    /// when only groups are selected, `strokeWidth` when nothing is stroked.
    pub fn properties(&self) -> Result<String, JsError> {
        let value = match self.session.properties().map_err(to_js)? {
            Some(properties) => properties_json(&properties),
            None => Value::Null,
        };
        to_json(&value)
    }

    /// Show a change to the selection without recording it: every move of a
    /// slider or a scrubbed number. `change` is JSON with one key — `x`,
    /// `y`, `width`, `height`, `rotation`, `opacity` (0–1), `fill` (a colour
    /// or `null`), `stroke` (`null`, or `{color, width}`), `strokeColor` or
    /// `strokeWidth`. False when there is nothing it could apply to.
    #[wasm_bindgen(js_name = previewProperty)]
    pub fn preview_property(&mut self, change: &str) -> Result<bool, JsError> {
        let property = parse_property(change)?;
        self.session.preview_property(property).map_err(to_js)
    }

    /// Record the previews as one undo step; true if anything changed.
    #[wasm_bindgen(js_name = commitProperty)]
    pub fn commit_property(&mut self) -> Result<bool, JsError> {
        self.session.commit_property().map_err(to_js)
    }

    /// Drop the previews, restoring what was there before them.
    #[wasm_bindgen(js_name = cancelProperty)]
    pub fn cancel_property(&mut self) -> Result<bool, JsError> {
        self.session.cancel_property().map_err(to_js)
    }

    /// Change the selection as one undo step: a typed value, a stepped one,
    /// a removed fill. Takes the same JSON as `previewProperty`.
    #[wasm_bindgen(js_name = setProperty)]
    pub fn set_property(&mut self, change: &str) -> Result<bool, JsError> {
        let property = parse_property(change)?;
        self.session.set_property(property).map_err(to_js)
    }

    // ---- Selection ---------------------------------------------------------------
    //
    // From here on, (x, y) is a screen point and `tolerance` a screen
    // distance, both in CSS pixels.

    /// A click with the select tool: "drag" (follow with `beginMove`), "hit"
    /// (Shift deselected a node; do nothing) or "miss" (follow with
    /// `beginMarquee`). `tolerance` is how far outside a shape still counts.
    #[wasm_bindgen(js_name = selectAt)]
    pub fn select_at(
        &mut self,
        x: f64,
        y: f64,
        additive: bool,
        tolerance: f64,
    ) -> Result<String, JsError> {
        let outcome = self
            .session
            .select_at(self.point(x, y), additive, self.distance(tolerance))
            .map_err(to_js)?;
        Ok(match outcome {
            SelectOutcome::Drag => "drag",
            SelectOutcome::Hit => "hit",
            SelectOutcome::Miss => "miss",
        }
        .to_owned())
    }

    /// A click on a layer-panel row.
    #[wasm_bindgen(js_name = selectLayer)]
    pub fn select_layer(&mut self, id: &str, additive: bool) -> Result<(), JsError> {
        let id = self.id(id)?;
        self.session.select_layer(id, additive).map_err(to_js)
    }

    #[wasm_bindgen(js_name = selectAll)]
    pub fn select_all(&mut self) -> Result<(), JsError> {
        self.session.select_all().map_err(to_js)
    }

    #[wasm_bindgen(js_name = clearSelection)]
    pub fn clear_selection(&mut self) -> Result<(), JsError> {
        self.session.clear_selection().map_err(to_js)
    }

    #[wasm_bindgen(js_name = selectionCount)]
    pub fn selection_count(&self) -> usize {
        self.session.selection().len()
    }

    /// Track the node under the pointer; true if that changed.
    pub fn hover(&mut self, x: f64, y: f64, tolerance: f64) -> Result<bool, JsError> {
        self.session
            .hover(self.point(x, y), self.distance(tolerance))
            .map_err(to_js)
    }

    #[wasm_bindgen(js_name = clearHover)]
    pub fn clear_hover(&mut self) -> bool {
        self.session.clear_hover()
    }

    /// Delete whatever the current mode has selected, as one undo step.
    #[wasm_bindgen(js_name = deleteSelection)]
    pub fn delete_selection(&mut self) -> Result<bool, JsError> {
        self.session.delete_selection().map_err(to_js)
    }

    /// Move the selection (or selected anchors) by (dx, dy) *document units*,
    /// as one undo step — a nudge is a unit whatever the zoom.
    pub fn nudge(&mut self, dx: f64, dy: f64) -> Result<bool, JsError> {
        self.session.nudge(Vec2::new(dx, dy)).map_err(to_js)
    }

    // ---- Gestures ------------------------------------------------------------------

    #[wasm_bindgen(js_name = beginMove)]
    pub fn begin_move(&mut self, x: f64, y: f64) -> Result<bool, JsError> {
        self.session
            .begin_transform(TransformKind::Move, self.point(x, y))
            .map_err(to_js)
    }

    /// Drag the selection handle at unit coordinates (u, v) of the frame.
    #[wasm_bindgen(js_name = beginScale)]
    pub fn begin_scale(&mut self, u: f64, v: f64, x: f64, y: f64) -> Result<bool, JsError> {
        self.session
            .begin_transform(TransformKind::Scale { u, v }, self.point(x, y))
            .map_err(to_js)
    }

    #[wasm_bindgen(js_name = beginRotate)]
    pub fn begin_rotate(&mut self, x: f64, y: f64) -> Result<bool, JsError> {
        self.session
            .begin_transform(TransformKind::Rotate, self.point(x, y))
            .map_err(to_js)
    }

    /// Start drawing a shape: `shape` is "rect" or "ellipse".
    #[wasm_bindgen(js_name = beginCreate)]
    pub fn begin_create(
        &mut self,
        shape: &str,
        x: f64,
        y: f64,
        srgb: &[u8],
    ) -> Result<(), JsError> {
        let shape = match shape {
            "rect" => ShapeKind::Rect,
            "ellipse" => ShapeKind::Ellipse,
            other => return Err(JsError::new(&format!("unknown shape {other:?}"))),
        };
        self.session
            .begin_create(shape, colour(srgb)?, self.point(x, y))
            .map_err(to_js)
    }

    #[wasm_bindgen(js_name = beginMarquee)]
    pub fn begin_marquee(&mut self, x: f64, y: f64, additive: bool) -> Result<(), JsError> {
        self.session
            .begin_marquee(self.point(x, y), additive)
            .map_err(to_js)
    }

    #[wasm_bindgen(js_name = updateGesture)]
    pub fn update_gesture(
        &mut self,
        x: f64,
        y: f64,
        shift: bool,
        alt: bool,
    ) -> Result<(), JsError> {
        self.session
            .update_gesture(self.point(x, y), Modifiers { shift, alt })
            .map_err(to_js)
    }

    /// Finish the gesture. Returns the new node's id when a shape was drawn.
    #[wasm_bindgen(js_name = endGesture)]
    pub fn end_gesture(&mut self) -> Result<Option<String>, JsError> {
        Ok(self.session.end_gesture().map_err(to_js)?.map(encode_id))
    }

    #[wasm_bindgen(js_name = cancelGesture)]
    pub fn cancel_gesture(&mut self) -> Result<bool, JsError> {
        self.session.cancel_gesture().map_err(to_js)
    }

    // ---- Pen and path editing ------------------------------------------------------

    /// A press with the pen tool. The first press starts a new path.
    #[wasm_bindgen(js_name = penPress)]
    pub fn pen_press(
        &mut self,
        x: f64,
        y: f64,
        shift: bool,
        tolerance: f64,
        srgb: &[u8],
    ) -> Result<(), JsError> {
        let stroke = Stroke {
            color: colour(srgb)?,
            width: PEN_STROKE_WIDTH,
        };
        self.session
            .pen_press(self.point(x, y), shift, self.distance(tolerance), stroke)
            .map_err(to_js)
    }

    #[wasm_bindgen(js_name = penDrag)]
    pub fn pen_drag(&mut self, x: f64, y: f64, shift: bool) -> Result<(), JsError> {
        self.session
            .pen_drag(self.point(x, y), shift)
            .map_err(to_js)
    }

    /// Returns the path's id when the press completed it (closed or ended).
    #[wasm_bindgen(js_name = penRelease)]
    pub fn pen_release(&mut self) -> Result<Option<String>, JsError> {
        Ok(self.session.pen_release().map_err(to_js)?.map(encode_id))
    }

    #[wasm_bindgen(js_name = penHover)]
    pub fn pen_hover(&mut self, x: f64, y: f64, tolerance: f64) -> bool {
        let (point, tolerance) = (self.point(x, y), self.distance(tolerance));
        self.session.pen_hover(point, tolerance)
    }

    #[wasm_bindgen(js_name = penFinish)]
    pub fn pen_finish(&mut self) -> Result<Option<String>, JsError> {
        Ok(self.session.pen_finish().map_err(to_js)?.map(encode_id))
    }

    /// Finish the pen path or stop editing a path; true if either was active.
    #[wasm_bindgen(js_name = finishMode)]
    pub fn finish_mode(&mut self) -> Result<bool, JsError> {
        self.session.finish_mode().map_err(to_js)
    }

    /// "pen", "path", or nothing.
    pub fn mode(&self) -> Option<String> {
        self.session.mode().map(|mode| mode_name(mode).to_owned())
    }

    #[wasm_bindgen(js_name = beginPathEdit)]
    pub fn begin_path_edit(&mut self) -> Result<bool, JsError> {
        self.session.begin_path_edit().map_err(to_js)
    }

    /// "handle", "anchor", "segment" (an anchor was inserted there) or "miss".
    #[wasm_bindgen(js_name = pathPress)]
    pub fn path_press(
        &mut self,
        x: f64,
        y: f64,
        tolerance: f64,
        additive: bool,
    ) -> Result<String, JsError> {
        let outcome = self
            .session
            .path_press(self.point(x, y), self.distance(tolerance), additive)
            .map_err(to_js)?;
        Ok(match outcome {
            PressOutcome::Handle => "handle",
            PressOutcome::Anchor => "anchor",
            PressOutcome::Segment => "segment",
            PressOutcome::Miss => "miss",
        }
        .to_owned())
    }

    #[wasm_bindgen(js_name = pathDrag)]
    pub fn path_drag(&mut self, x: f64, y: f64, shift: bool, alt: bool) -> Result<(), JsError> {
        self.session
            .path_drag(self.point(x, y), Modifiers { shift, alt })
            .map_err(to_js)
    }

    #[wasm_bindgen(js_name = pathRelease)]
    pub fn path_release(&mut self) -> Result<bool, JsError> {
        self.session.path_release().map_err(to_js)
    }

    #[wasm_bindgen(js_name = pathCancelDrag)]
    pub fn path_cancel_drag(&mut self) -> Result<(), JsError> {
        self.session.path_cancel_drag().map_err(to_js)
    }

    /// Toggles corner/smooth on an anchor; returns whether editing continues.
    #[wasm_bindgen(js_name = pathDoubleClick)]
    pub fn path_double_click(&mut self, x: f64, y: f64, tolerance: f64) -> Result<bool, JsError> {
        self.session
            .path_double_click(self.point(x, y), self.distance(tolerance))
            .map_err(to_js)
    }

    // ---- Overlay -------------------------------------------------------------------

    /// Everything drawn over the artwork, as JSON, in screen pixels — plus
    /// the artboard's place on screen. Sizes a label shows (frame width and
    /// height, the artboard's size) stay in document units. One call per
    /// frame, not one per node.
    pub fn overlay(&self) -> Result<String, JsError> {
        let overlay = self.session.overlay().map_err(to_js)?;
        let artboard = self.session.document().artboard();
        to_json(&overlay_json(&overlay, self.view.to_screen(), artboard))
    }
}

impl Editor {
    fn insert(&mut self, node: Node) -> Result<String, JsError> {
        Ok(encode_id(self.session.insert(node).map_err(to_js)?))
    }

    fn id(&self, id: &str) -> Result<NodeId, JsError> {
        let raw: u64 = id.parse().map_err(|_| JsError::new("malformed node id"))?;
        let id = NodeId::from(KeyData::from_ffi(raw));
        if !self.session.document().contains(id) {
            return Err(JsError::new("unknown node id"));
        }
        Ok(id)
    }

    /// A screen point, in the document.
    fn point(&self, x: f64, y: f64) -> Point {
        self.view.screen_to_document(Point::new(x, y))
    }

    /// A screen distance, in document units.
    fn distance(&self, screen: f64) -> f64 {
        self.view.screen_distance_to_document(screen)
    }

    fn centre(&self) -> Point {
        let size = self.view.screen_size();
        Point::new(size.width / 2.0, size.height / 2.0)
    }

    fn fit(&mut self, max_zoom: f64) {
        let artboard = self.session.document().artboard();
        let area = Rect::new(0.0, 0.0, artboard.width, artboard.height);
        self.view.fit(area, FIT_PADDING, max_zoom);
        self.fit_pending = false;
    }

    /// Match the pixmap to the viewport, dropping the pixels if it changes.
    fn sync_pixmap(&mut self) -> Result<(), JsError> {
        if (self.pixmap.width(), self.pixmap.height()) != self.view.device_size() {
            self.pixmap = pixmap_for(&self.view)?;
            self.shown = None;
        }
        Ok(())
    }
}

fn overlay_json(overlay: &Overlay, to_screen: Affine, artboard: Size) -> Value {
    let point = |p: Point| {
        let s = to_screen * p;
        [s.x, s.y]
    };
    let line = |&(a, b): &(Point, Point)| {
        let (a, b) = (to_screen * a, to_screen * b);
        [a.x, a.y, b.x, b.y]
    };
    let path = |p: &BezPath| {
        let mut p = p.clone();
        p.apply_affine(to_screen);
        p.to_svg()
    };
    let rect = |r: Rect| {
        let r = to_screen.transform_rect_bbox(r);
        [r.x0, r.y0, r.x1, r.y1]
    };
    let frame = overlay.frame.map(|frame| {
        let (width, height) = frame.size();
        json!({
            "corners": frame.corners().map(point),
            "width": width,
            "height": height,
        })
    });
    let mut value = json!({
        "mode": overlay.mode.map(mode_name),
        "frame": frame,
        "outlines": overlay.outlines.iter().map(path).collect::<Vec<_>>(),
        "hover": overlay.hover.as_ref().map(path),
        "marquee": overlay.marquee.map(rect),
        "locked": overlay.locked,
        "gesture": overlay.gesture,
        "artboard": {
            "rect": rect(Rect::new(0.0, 0.0, artboard.width, artboard.height)),
            "width": artboard.width,
            "height": artboard.height,
        },
    });
    if let Some(pen) = &overlay.pen {
        value["pen"] = json!({
            "anchors": pen.anchors.iter().copied().map(point).collect::<Vec<_>>(),
            "handles": pen.handles.iter().map(line).collect::<Vec<_>>(),
            "preview": pen.preview.as_ref().map(path),
            "closable": pen.closable,
        });
    }
    if let Some(edit) = &overlay.path {
        value["path"] = json!({
            "outline": path(&edit.outline),
            "anchors": edit.anchors.iter().map(|&(at, selected)| json!({
                "at": point(at),
                "selected": selected,
            })).collect::<Vec<_>>(),
            "handles": edit.handles.iter().map(line).collect::<Vec<_>>(),
        });
    }
    value
}

fn properties_json(p: &Properties) -> Value {
    fn shared<T>(value: Shared<T>, to_value: impl Fn(T) -> Value) -> Value {
        match value {
            Shared::Same(v) => to_value(v),
            Shared::Mixed => json!("mixed"),
        }
    }
    let colour = |c: Option<LinearRgba>| c.map_or(Value::Null, |c| json!(c.to_srgb8()));
    let mut out = json!({
        "count": p.count,
        "x": p.x,
        "y": p.y,
        "width": p.width,
        "height": p.height,
        "rotation": p.rotation,
        "opacity": shared(p.opacity, |o| json!(o)),
    });
    if let Some(fill) = p.fill {
        out["fill"] = shared(fill, colour);
    }
    if let Some(stroke) = p.stroke {
        out["stroke"] = shared(stroke, colour);
    }
    if let Some(width) = p.stroke_width {
        out["strokeWidth"] = shared(width, |w| json!(w));
    }
    out
}

/// A property change from the page: JSON with exactly one key. Parsed by
/// hand from a `Value`: a derived enum does the same in twice the wasm.
fn parse_property(change: &str) -> Result<Property, JsError> {
    let invalid = |why: &str| JsError::new(&format!("invalid property change: {why}"));
    let value: Value = serde_json::from_str(change).map_err(|e| invalid(&e.to_string()))?;
    let Some((key, value)) = value
        .as_object()
        .filter(|o| o.len() == 1)
        .and_then(|o| o.iter().next())
    else {
        return Err(invalid("expected an object with one key"));
    };
    let number = || value.as_f64().ok_or_else(|| invalid("expected a number"));
    let colour =
        |value: &Value| colour_value(value).ok_or_else(|| invalid("expected [r, g, b, a]"));
    Ok(match key.as_str() {
        "x" => Property::X(number()?),
        "y" => Property::Y(number()?),
        "width" => Property::Width(number()?),
        "height" => Property::Height(number()?),
        "rotation" => Property::Rotation(number()?),
        "opacity" => Property::Opacity(number()? as f32),
        "fill" if value.is_null() => Property::Fill(None),
        "fill" => Property::Fill(Some(colour(value)?)),
        "stroke" if value.is_null() => Property::Stroke(None),
        "stroke" => Property::Stroke(Some(Stroke {
            color: colour(&value["color"])?,
            width: value["width"]
                .as_f64()
                .ok_or_else(|| invalid("a stroke needs a width"))?,
        })),
        "strokeColor" => Property::StrokeColor(colour(value)?),
        "strokeWidth" => Property::StrokeWidth(number()?),
        _ => return Err(invalid(&format!("unknown property {key}"))),
    })
}

/// `[r, g, b, a]` in sRGB bytes.
fn colour_value(value: &Value) -> Option<LinearRgba> {
    let byte = |c: &Value| c.as_u64().and_then(|c| u8::try_from(c).ok());
    match value.as_array()?.as_slice() {
        [r, g, b, a] => Some(LinearRgba::from_srgb8(
            byte(r)?,
            byte(g)?,
            byte(b)?,
            byte(a)?,
        )),
        _ => None,
    }
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Pen => "pen",
        Mode::PathEdit => "path",
    }
}

/// A pixmap covering the viewport in device pixels.
fn pixmap_for(view: &View) -> Result<Pixmap, JsError> {
    let (width, height) = view.device_size();
    Pixmap::new(width, height).ok_or_else(|| JsError::new("invalid viewport size"))
}

fn colour(srgb: &[u8]) -> Result<LinearRgba, JsError> {
    let &[r, g, b, a] = srgb else {
        return Err(JsError::new("colour must be 4 sRGB bytes"));
    };
    Ok(LinearRgba::from_srgb8(r, g, b, a))
}

/// Node ids cross the boundary as opaque strings.
///
/// Encoded from the slotmap key rather than searched for: id lookup runs on
/// every frame of a drag, so it has to be O(1). The UI must treat these as
/// opaque and only echo back what it was given.
fn encode_id(id: NodeId) -> String {
    id.data().as_ffi().to_string()
}

fn to_json(value: &Value) -> Result<String, JsError> {
    serde_json::to_string(value).map_err(|e| JsError::new(&e.to_string()))
}

fn to_js(e: graphicgene_core::error::CoreError) -> JsError {
    JsError::new(&e.to_string())
}
