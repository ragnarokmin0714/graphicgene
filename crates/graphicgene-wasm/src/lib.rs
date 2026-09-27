//! wasm-bindgen bindings: a thin shell over `graphicgene_core::Session`.
//!
//! This is the batching boundary. One call per user interaction, never one
//! call per node — a chatty WASM boundary is the most common way a project
//! like this ends up slow, and it is very hard to undo once the UI depends on
//! the fine-grained shape.
//!
//! Nothing here decides how editing behaves; that is all in the session, so
//! a desktop shell gets the same rules for free. What lives here is only what
//! is specific to JavaScript: node ids as strings, colours as sRGB bytes,
//! results as JSON, and the pixel buffer the canvas is painted from.
//!
//! Pixels never cross the boundary by copy. `render` redraws only what
//! changed and says where; the page reads that part in place from wasm
//! memory through `pixelsPtr`.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::command::Command;
use graphicgene_core::geom::{Affine, BezPath, Ellipse, Point, Rect, Shape, Size, Vec2};
use graphicgene_core::gesture::{Modifiers, ShapeKind, TransformKind};
use graphicgene_core::node::{Node, NodeId, Stroke};
use graphicgene_core::path_edit::PressOutcome;
use graphicgene_core::session::{Mode, Overlay, SelectOutcome, Session};
use graphicgene_render::{CpuRenderer, Damage, PixelRect, RenderScene, Renderer};
use serde_json::{Value, json};
use slotmap::{Key, KeyData};
use tiny_skia::Pixmap;
use wasm_bindgen::prelude::*;

/// Stroke width for paths drawn with the pen, in document units.
const PEN_STROKE_WIDTH: f64 = 2.0;

/// Everything the UI talks to.
#[wasm_bindgen]
pub struct Editor {
    session: Session,
    renderer: CpuRenderer,
    pixmap: Pixmap,
    scene: RenderScene,
}

#[wasm_bindgen]
impl Editor {
    /// A new document with a `width` × `height` artboard. A document loaded
    /// later brings its own size.
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32) -> Result<Editor, JsError> {
        let session = Session::with_artboard(Size::new(width.into(), height.into()));
        let mut scene = RenderScene::default();
        scene.background = Some(LinearRgba::WHITE);
        Ok(Editor {
            pixmap: pixmap_for(&session)?,
            session,
            renderer: CpuRenderer::new(),
            scene,
        })
    }

    /// The artboard's width in pixels: the size the canvas must be.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.pixmap.height()
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

    /// Replace the document. Its artboard may be a different size, so read
    /// `width` and `height` again afterwards.
    #[wasm_bindgen(js_name = loadJson)]
    pub fn load_json(&mut self, text: &str) -> Result<(), JsError> {
        self.session.load(text).map_err(to_js)?;
        let pixmap = pixmap_for(&self.session)?;
        if (pixmap.width(), pixmap.height()) != (self.pixmap.width(), self.pixmap.height()) {
            self.pixmap = pixmap;
        }
        Ok(())
    }

    /// The artwork as SVG, the size of the artboard.
    #[wasm_bindgen(js_name = exportSvg)]
    pub fn export_svg(&self) -> Result<String, JsError> {
        self.session.export_svg().map_err(to_js)
    }

    // ---- Pixels ------------------------------------------------------------------

    /// Bring the pixels up to date and return the area that changed, as
    /// `[x, y, width, height]` in pixels — all zeros when nothing did, as
    /// after a selection click or a hover.
    ///
    /// Only that area is redrawn. Read the pixels in place with `pixelsPtr`.
    pub fn render(&mut self) -> Result<Vec<u32>, JsError> {
        let changes = self.session.prepare_render().map_err(to_js)?;
        let damage = self
            .scene
            .update(self.session.document(), &changes)
            .map_err(to_js)?;
        let (width, height) = (self.pixmap.width(), self.pixmap.height());
        let dirty = match damage {
            Damage::None => return Ok(vec![0; 4]),
            Damage::Region(region) => region,
            Damage::Everything => Rect::new(0.0, 0.0, width.into(), height.into()),
        };
        self.renderer
            .render(&self.scene, dirty, &mut self.pixmap)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(match PixelRect::covering(dirty, width, height) {
            Some(rect) => vec![rect.x, rect.y, rect.width, rect.height],
            None => vec![0; 4],
        })
    }

    /// Where the canvas pixels start in wasm memory: `width * height * 4`
    /// bytes of RGBA, premultiplied. The same address until the artboard
    /// changes size; the page must still rebuild its view whenever wasm
    /// memory grows, which detaches the old buffer.
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

    // ---- Selection ---------------------------------------------------------------

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
            .select_at(Point::new(x, y), additive, tolerance)
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
            .hover(Point::new(x, y), tolerance)
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

    /// Move the selection (or selected anchors) by (dx, dy), as one undo step.
    pub fn nudge(&mut self, dx: f64, dy: f64) -> Result<bool, JsError> {
        self.session.nudge(Vec2::new(dx, dy)).map_err(to_js)
    }

    // ---- Gestures ------------------------------------------------------------------

    #[wasm_bindgen(js_name = beginMove)]
    pub fn begin_move(&mut self, x: f64, y: f64) -> Result<bool, JsError> {
        self.session
            .begin_transform(TransformKind::Move, Point::new(x, y))
            .map_err(to_js)
    }

    /// Drag the selection handle at unit coordinates (u, v) of the frame.
    #[wasm_bindgen(js_name = beginScale)]
    pub fn begin_scale(&mut self, u: f64, v: f64, x: f64, y: f64) -> Result<bool, JsError> {
        self.session
            .begin_transform(TransformKind::Scale { u, v }, Point::new(x, y))
            .map_err(to_js)
    }

    #[wasm_bindgen(js_name = beginRotate)]
    pub fn begin_rotate(&mut self, x: f64, y: f64) -> Result<bool, JsError> {
        self.session
            .begin_transform(TransformKind::Rotate, Point::new(x, y))
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
            .begin_create(shape, colour(srgb)?, Point::new(x, y))
            .map_err(to_js)
    }

    #[wasm_bindgen(js_name = beginMarquee)]
    pub fn begin_marquee(&mut self, x: f64, y: f64, additive: bool) -> Result<(), JsError> {
        self.session
            .begin_marquee(Point::new(x, y), additive)
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
            .update_gesture(Point::new(x, y), Modifiers { shift, alt })
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
    //
    // `tolerance` is a pick distance in document units: a screen distance the
    // UI divides by the zoom, like its handle sizes.

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
            .pen_press(Point::new(x, y), shift, tolerance, stroke)
            .map_err(to_js)
    }

    #[wasm_bindgen(js_name = penDrag)]
    pub fn pen_drag(&mut self, x: f64, y: f64, shift: bool) -> Result<(), JsError> {
        self.session
            .pen_drag(Point::new(x, y), shift)
            .map_err(to_js)
    }

    /// Returns the path's id when the press completed it (closed or ended).
    #[wasm_bindgen(js_name = penRelease)]
    pub fn pen_release(&mut self) -> Result<Option<String>, JsError> {
        Ok(self.session.pen_release().map_err(to_js)?.map(encode_id))
    }

    #[wasm_bindgen(js_name = penHover)]
    pub fn pen_hover(&mut self, x: f64, y: f64, tolerance: f64) -> bool {
        self.session.pen_hover(Point::new(x, y), tolerance)
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
            .path_press(Point::new(x, y), tolerance, additive)
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
            .path_drag(Point::new(x, y), Modifiers { shift, alt })
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
            .path_double_click(Point::new(x, y), tolerance)
            .map_err(to_js)
    }

    // ---- Overlay -------------------------------------------------------------------

    /// Everything drawn over the artwork, as JSON, in document units. One
    /// call per frame, not one per node.
    pub fn overlay(&self) -> Result<String, JsError> {
        let overlay = self.session.overlay().map_err(to_js)?;
        to_json(&overlay_json(&overlay))
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
}

fn overlay_json(overlay: &Overlay) -> Value {
    let point = |p: Point| [p.x, p.y];
    let line = |(a, b): &(Point, Point)| [a.x, a.y, b.x, b.y];
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
        "outlines": overlay.outlines.iter().map(BezPath::to_svg).collect::<Vec<_>>(),
        "hover": overlay.hover.as_ref().map(BezPath::to_svg),
        "marquee": overlay.marquee.map(|r| [r.x0, r.y0, r.x1, r.y1]),
        "gesture": overlay.gesture,
    });
    if let Some(pen) = &overlay.pen {
        value["pen"] = json!({
            "anchors": pen.anchors.iter().copied().map(point).collect::<Vec<_>>(),
            "handles": pen.handles.iter().map(line).collect::<Vec<_>>(),
            "preview": pen.preview.as_ref().map(BezPath::to_svg),
            "closable": pen.closable,
        });
    }
    if let Some(path) = &overlay.path {
        value["path"] = json!({
            "outline": path.outline.to_svg(),
            "anchors": path.anchors.iter().map(|&(at, selected)| json!({
                "at": point(at),
                "selected": selected,
            })).collect::<Vec<_>>(),
            "handles": path.handles.iter().map(line).collect::<Vec<_>>(),
        });
    }
    value
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Pen => "pen",
        Mode::PathEdit => "path",
    }
}

/// A pixmap covering the artboard at one pixel per document unit.
fn pixmap_for(session: &Session) -> Result<Pixmap, JsError> {
    let size = session.document().artboard();
    Pixmap::new(size.width.ceil() as u32, size.height.ceil() as u32)
        .ok_or_else(|| JsError::new("invalid artboard size"))
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
