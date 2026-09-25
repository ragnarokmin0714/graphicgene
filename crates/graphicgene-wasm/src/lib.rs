//! wasm-bindgen bindings.
//!
//! This is the batching boundary. One call per user interaction, never one
//! call per node — a chatty WASM boundary is the most common way a project
//! like this ends up slow, and it is very hard to undo once the UI depends on
//! the fine-grained shape.
//!
//! The document lives here, in Rust. React renders a view of it and sends
//! commands back; it never holds authoritative document state. The moment it
//! does, the web and desktop apps start behaving differently and the core
//! stops being the product.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::command::{Command, Journal};
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Affine, BezPath, Point, Rect, Shape, Vec2};
use graphicgene_core::gesture::{self, Frame, Gesture, Modifiers, ShapeKind, TransformKind};
use graphicgene_core::hit;
use graphicgene_core::node::{Node, NodeId, NodeKind, Stroke};
use graphicgene_core::path_edit::{DeleteOutcome, PathEdit, PressOutcome};
use graphicgene_core::pen::PenSession;
use graphicgene_core::project::Project;
use graphicgene_core::selection::Selection;
use graphicgene_render::{CpuRenderer, RenderScene, Renderer};
use slotmap::{Key, KeyData};
use tiny_skia::Pixmap;
use wasm_bindgen::prelude::*;

/// Everything the UI talks to.
#[wasm_bindgen]
pub struct Editor {
    document: Document,
    journal: Journal,
    renderer: CpuRenderer,
    pixmap: Pixmap,
    scene: RenderScene,
    /// Set when the document changed but the scene has not been rebuilt yet.
    scene_dirty: bool,
    selection: Selection,
    /// The drag in progress, if any. At most one at a time.
    gesture: Option<Gesture>,
    /// The node under the pointer, for the hover outline.
    hover: Option<NodeId>,
    /// A path being drawn with the pen, if any.
    pen: Option<PenSession>,
    /// The path whose anchors are being edited, if any. Never at the same
    /// time as `pen`.
    path_edit: Option<PathEdit>,
}

/// How far outside a shape a click still hits it, in document units.
/// When zoom arrives this becomes a screen distance divided by the zoom.
const HIT_TOLERANCE: f64 = 4.0;
/// Stroke width for paths drawn with the pen, in document units.
const PEN_STROKE_WIDTH: f64 = 2.0;

#[wasm_bindgen]
impl Editor {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32) -> Result<Editor, JsError> {
        let pixmap =
            Pixmap::new(width, height).ok_or_else(|| JsError::new("invalid canvas size"))?;
        Ok(Editor {
            document: Document::new(),
            journal: Journal::new(),
            renderer: CpuRenderer::new(),
            pixmap,
            scene: RenderScene::default(),
            scene_dirty: true,
            selection: Selection::new(),
            gesture: None,
            hover: None,
            pen: None,
            path_edit: None,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), JsError> {
        self.pixmap =
            Pixmap::new(width, height).ok_or_else(|| JsError::new("invalid canvas size"))?;
        Ok(())
    }

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
        let fill = fill_from_bytes(srgb)?;
        let path = Rect::new(x, y, x + width, y + height).to_path(0.1);
        self.insert(Node::vector("Rectangle", path, Some(fill)))
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
        let fill = fill_from_bytes(srgb)?;
        let path = graphicgene_core::geom::Ellipse::new((cx, cy), (rx, ry), 0.0).to_path(0.1);
        self.insert(Node::vector("Ellipse", path, Some(fill)))
    }

    /// Add a path from an SVG path string, so the pen tool does not need its
    /// own binding per segment.
    #[wasm_bindgen(js_name = addPath)]
    pub fn add_path(&mut self, svg_path: &str, srgb: &[u8]) -> Result<String, JsError> {
        let fill = fill_from_bytes(srgb)?;
        let path = BezPath::from_svg(svg_path).map_err(|e| JsError::new(&e.to_string()))?;
        self.insert(Node::vector("Path", path, Some(fill)))
    }

    #[wasm_bindgen(js_name = setTransform)]
    pub fn set_transform(&mut self, id: &str, m: &[f64]) -> Result<(), JsError> {
        if m.len() != 6 {
            return Err(JsError::new("transform needs 6 coefficients"));
        }
        let id = parse_id(&self.document, id)?;
        let transform = Affine::new([m[0], m[1], m[2], m[3], m[4], m[5]]);
        self.execute(Command::SetTransform { id, transform })
    }

    /// Undo. While the pen is drawing, this removes the last anchor instead:
    /// the unfinished path is not in the journal yet.
    pub fn undo(&mut self) -> Result<bool, JsError> {
        self.cancel_gesture()?;
        if let Some(pen) = self.pen.as_mut() {
            if !pen.undo_anchor(&mut self.document).map_err(to_js)? {
                let pen = self.pen.take().expect("checked above");
                pen.cancel(&mut self.document, &mut self.selection)
                    .map_err(to_js)?;
            }
            self.scene_dirty = true;
            return Ok(true);
        }
        self.cancel_path_drag()?;
        let changed = self.journal.undo(&mut self.document).map_err(to_js)?;
        self.after_history_change(changed)?;
        Ok(changed)
    }

    pub fn redo(&mut self) -> Result<bool, JsError> {
        if self.pen.is_some() {
            return Ok(false);
        }
        self.cancel_gesture()?;
        self.cancel_path_drag()?;
        let changed = self.journal.redo(&mut self.document).map_err(to_js)?;
        self.after_history_change(changed)?;
        Ok(changed)
    }

    #[wasm_bindgen(js_name = canUndo)]
    pub fn can_undo(&self) -> bool {
        self.pen.is_some() || self.journal.can_undo()
    }

    #[wasm_bindgen(js_name = canRedo)]
    pub fn can_redo(&self) -> bool {
        self.pen.is_none() && self.journal.can_redo()
    }

    /// Render and return RGBA bytes for the whole canvas.
    ///
    /// One copy per frame across the boundary. When this becomes the
    /// bottleneck the fix is a shared buffer, not a chattier API.
    pub fn render(&mut self) -> Result<Vec<u8>, JsError> {
        if self.scene_dirty {
            graphicgene_core::layout::run(&mut self.document).map_err(to_js)?;
            self.scene = RenderScene::build(&self.document).map_err(to_js)?;
            self.scene_dirty = false;
        }
        self.pixmap.fill(tiny_skia::Color::WHITE);
        let dirty = Rect::new(
            0.0,
            0.0,
            self.pixmap.width() as f64,
            self.pixmap.height() as f64,
        );
        self.renderer
            .render(&self.scene, dirty, &mut self.pixmap)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(self.pixmap.data().to_vec())
    }

    /// Serialize the project. Core does no IO — the caller decides where these
    /// bytes go (IndexedDB on web, `std::fs` on desktop).
    ///
    /// The copy that is written drops detached nodes. The live document keeps
    /// them, since undo depends on them, but the file has no undo history, so
    /// saving them would only make every autosave larger than the last.
    #[wasm_bindgen(js_name = toJson)]
    pub fn to_json(&self) -> Result<String, JsError> {
        let mut document = self.document.clone();
        document.purge_unreachable();
        Project::new(document).to_json().map_err(to_js)
    }

    /// The document as SVG, `width` × `height` in size.
    #[wasm_bindgen(js_name = exportSvg)]
    pub fn export_svg(&self, width: f64, height: f64) -> Result<String, JsError> {
        graphicgene_core::svg::to_svg(&self.document, width, height).map_err(to_js)
    }

    /// Whether a press is in progress, so the document holds a preview that
    /// should not be saved yet: a drag, a pen path, or a path-edit drag.
    pub fn busy(&self) -> bool {
        self.gesture.is_some()
            || self.pen.is_some()
            || self.path_edit.as_ref().is_some_and(PathEdit::is_dragging)
    }

    #[wasm_bindgen(js_name = loadJson)]
    pub fn load_json(&mut self, text: &str) -> Result<(), JsError> {
        let project = Project::from_json(text).map_err(to_js)?;
        self.gesture = None;
        self.pen = None;
        self.path_edit = None;
        self.document = project.document;
        self.journal.clear();
        self.selection.clear();
        self.hover = None;
        self.scene_dirty = true;
        Ok(())
    }

    /// The layer tree, as JSON, for the UI to render.
    ///
    /// Rows come in layer-panel order, not paint order: the root is omitted
    /// and the topmost sibling comes first, with each group directly above its
    /// own children. `depth` is 0 for the root's children.
    #[wasm_bindgen(js_name = layerTree)]
    pub fn layer_tree(&self) -> Result<String, JsError> {
        let root = self.document.root();
        let mut out = Vec::new();
        // Children are stored bottom-to-top, so pushing them in stored order
        // pops the topmost one first.
        let mut stack: Vec<(NodeId, u32)> = self
            .document
            .children_of(root)
            .map_err(to_js)?
            .iter()
            .map(|&id| (id, 0))
            .collect();
        while let Some((id, depth)) = stack.pop() {
            let node = self.document.get(id).map_err(to_js)?;
            if let Some(children) = node.children() {
                stack.extend(children.iter().map(|&child| (child, depth + 1)));
            }
            out.push(serde_json::json!({
                "id": encode_id(id),
                "depth": depth,
                "selected": self.selection.contains(id),
                "name": node.common.name,
                "visible": node.common.visible,
                "locked": node.common.locked,
                "opacity": node.common.opacity,
                "kind": match node.kind {
                    graphicgene_core::node::NodeKind::Group(_) => "group",
                    graphicgene_core::node::NodeKind::Vector(_) => "vector",
                },
            }));
        }
        serde_json::to_string(&out).map_err(|e| JsError::new(&e.to_string()))
    }

    // ---- Selection -------------------------------------------------------
    //
    // Selection lives here rather than in React because it drives transforms.
    // It is not document state: not saved, not part of undo.

    /// A click with the select tool at document point (x, y). Returns what the
    /// press should turn into:
    ///
    /// - `"drag"`: it landed on a node that is now selected; follow with
    ///   `beginMove`.
    /// - `"hit"`: it landed on a node but Shift deselected it; do nothing.
    /// - `"miss"`: it landed on empty space; follow with `beginMarquee`.
    ///
    /// "hit" has to be told apart from "miss": a marquee started on the node
    /// just deselected would pick it straight back up on the first jitter.
    #[wasm_bindgen(js_name = selectAt)]
    pub fn select_at(&mut self, x: f64, y: f64, additive: bool) -> Result<String, JsError> {
        let hit = hit::hit_test(&self.document, Point::new(x, y), HIT_TOLERANCE).map_err(to_js)?;
        let outcome = match (hit, self.selection.click(hit, additive)) {
            (_, true) => "drag",
            (Some(_), false) => "hit",
            (None, false) => "miss",
        };
        Ok(outcome.to_owned())
    }

    /// A click on a layer-panel row.
    #[wasm_bindgen(js_name = selectLayer)]
    pub fn select_layer(&mut self, id: &str, additive: bool) -> Result<(), JsError> {
        let id = parse_id(&self.document, id)?;
        self.finish_mode()?;
        if additive {
            self.selection.toggle(id);
        } else {
            self.selection.set([id]);
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = selectAll)]
    pub fn select_all(&mut self) -> Result<(), JsError> {
        self.finish_mode()?;
        let ids = hit::selectable(&self.document).map_err(to_js)?;
        self.selection.set(ids);
        Ok(())
    }

    #[wasm_bindgen(js_name = clearSelection)]
    pub fn clear_selection(&mut self) -> Result<(), JsError> {
        self.finish_mode()?;
        self.selection.clear();
        Ok(())
    }

    #[wasm_bindgen(js_name = selectionCount)]
    pub fn selection_count(&self) -> usize {
        self.selection.len()
    }

    /// Track the node under the pointer. Returns whether that changed, so the
    /// UI only redraws the overlay when it has to.
    pub fn hover(&mut self, x: f64, y: f64) -> Result<bool, JsError> {
        let hit = hit::hit_test(&self.document, Point::new(x, y), HIT_TOLERANCE).map_err(to_js)?;
        let changed = hit != self.hover;
        self.hover = hit;
        Ok(changed)
    }

    #[wasm_bindgen(js_name = clearHover)]
    pub fn clear_hover(&mut self) -> bool {
        self.hover.take().is_some()
    }

    /// Delete, as one undo step, whatever the current mode has selected:
    /// the last pen anchor while drawing, the selected anchors while editing
    /// a path, otherwise the selected nodes.
    #[wasm_bindgen(js_name = deleteSelection)]
    pub fn delete_selection(&mut self) -> Result<bool, JsError> {
        if self.pen.is_some() {
            return self.undo();
        }
        if let Some(edit) = self.path_edit.as_mut() {
            let outcome = edit
                .delete_selected(&mut self.document, &mut self.journal)
                .map_err(to_js)?;
            match outcome {
                DeleteOutcome::Nothing => return Ok(false),
                DeleteOutcome::Anchors => {}
                DeleteOutcome::EmptiedPath => {
                    let id = edit.id();
                    self.path_edit = None;
                    self.execute(Command::Detach { id })?;
                    self.selection.clear();
                }
            }
            self.scene_dirty = true;
            return Ok(true);
        }
        if self.selection.is_empty() {
            return Ok(false);
        }
        let commands = self
            .selection
            .ids()
            .iter()
            .map(|&id| Command::Detach { id })
            .collect();
        self.execute(Command::Batch(commands))?;
        self.selection.clear();
        self.hover = None;
        Ok(true)
    }

    /// Move the selection by (dx, dy) document units, as one undo step.
    pub fn nudge(&mut self, dx: f64, dy: f64) -> Result<bool, JsError> {
        if self.pen.is_some() || self.gesture.is_some() {
            return Ok(false);
        }
        if let Some(edit) = self.path_edit.as_mut() {
            let moved = edit
                .nudge(&mut self.document, &mut self.journal, Vec2::new(dx, dy))
                .map_err(to_js)?;
            self.scene_dirty |= moved;
            return Ok(moved);
        }
        if self.selection.is_empty() {
            return Ok(false);
        }
        let command = gesture::world_delta_command(
            &self.document,
            self.selection.ids(),
            Affine::translate((dx, dy)),
        )
        .map_err(to_js)?;
        self.execute(command)?;
        Ok(true)
    }

    // ---- Gestures ----------------------------------------------------------
    //
    // One press-drag-release: begin* on pointer down, updateGesture on every
    // move, endGesture on release, cancelGesture on Escape or a lost pointer.
    // The drag previews in the document and lands in the journal as one step.

    /// Start dragging the selection. Returns false if there is nothing to move.
    #[wasm_bindgen(js_name = beginMove)]
    pub fn begin_move(&mut self, x: f64, y: f64) -> Result<bool, JsError> {
        self.begin_transform(TransformKind::Move, x, y)
    }

    /// Start dragging the selection handle at unit coordinates (u, v) of the
    /// selection frame; see `overlay` for where the frame is.
    #[wasm_bindgen(js_name = beginScale)]
    pub fn begin_scale(&mut self, u: f64, v: f64, x: f64, y: f64) -> Result<bool, JsError> {
        self.begin_transform(TransformKind::Scale { u, v }, x, y)
    }

    #[wasm_bindgen(js_name = beginRotate)]
    pub fn begin_rotate(&mut self, x: f64, y: f64) -> Result<bool, JsError> {
        self.begin_transform(TransformKind::Rotate, x, y)
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
        let fill = fill_from_bytes(srgb)?;
        self.cancel_gesture()?;
        let gesture = Gesture::create(
            &mut self.document,
            &mut self.selection,
            shape,
            fill,
            Point::new(x, y),
        )
        .map_err(to_js)?;
        self.gesture = Some(gesture);
        self.scene_dirty = true;
        Ok(())
    }

    #[wasm_bindgen(js_name = beginMarquee)]
    pub fn begin_marquee(&mut self, x: f64, y: f64, additive: bool) -> Result<(), JsError> {
        self.cancel_gesture()?;
        self.gesture = Some(Gesture::marquee(
            &self.selection,
            Point::new(x, y),
            additive,
        ));
        Ok(())
    }

    /// Follow the pointer. A no-op when no gesture is active.
    #[wasm_bindgen(js_name = updateGesture)]
    pub fn update_gesture(
        &mut self,
        x: f64,
        y: f64,
        shift: bool,
        alt: bool,
    ) -> Result<(), JsError> {
        let Some(gesture) = self.gesture.as_mut() else {
            return Ok(());
        };
        gesture
            .update(
                &mut self.document,
                &mut self.selection,
                Point::new(x, y),
                Modifiers { shift, alt },
            )
            .map_err(to_js)?;
        self.scene_dirty = true;
        Ok(())
    }

    /// Finish the gesture. Returns the new node's id when a shape was drawn.
    #[wasm_bindgen(js_name = endGesture)]
    pub fn end_gesture(&mut self) -> Result<Option<String>, JsError> {
        let Some(gesture) = self.gesture.take() else {
            return Ok(None);
        };
        let created = gesture
            .commit(&mut self.document, &mut self.journal, &mut self.selection)
            .map_err(to_js)?;
        self.scene_dirty = true;
        Ok(created.map(encode_id))
    }

    /// Abandon the gesture. Returns whether one was active.
    #[wasm_bindgen(js_name = cancelGesture)]
    pub fn cancel_gesture(&mut self) -> Result<bool, JsError> {
        let Some(gesture) = self.gesture.take() else {
            return Ok(false);
        };
        gesture
            .cancel(&mut self.document, &mut self.selection)
            .map_err(to_js)?;
        self.scene_dirty = true;
        Ok(true)
    }

    // ---- Pen -----------------------------------------------------------------
    //
    // Presses place anchors, drags pull out handles, and the path is one undo
    // step once finished. `tolerance` is how near to an existing end anchor
    // counts as pressing it, in document units — a screen distance the UI
    // converts, like handle sizes.

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
        let point = Point::new(x, y);
        let modifiers = Modifiers { shift, alt: false };
        match self.pen.as_mut() {
            Some(pen) => pen
                .press(&mut self.document, point, modifiers, tolerance)
                .map_err(to_js)?,
            None => {
                self.cancel_gesture()?;
                self.end_path_edit()?;
                let stroke = Stroke {
                    color: fill_from_bytes(srgb)?,
                    width: PEN_STROKE_WIDTH,
                };
                let pen = PenSession::start(&mut self.document, &mut self.selection, stroke, point)
                    .map_err(to_js)?;
                self.pen = Some(pen);
            }
        }
        self.hover = None;
        self.scene_dirty = true;
        Ok(())
    }

    #[wasm_bindgen(js_name = penDrag)]
    pub fn pen_drag(&mut self, x: f64, y: f64, shift: bool) -> Result<(), JsError> {
        let Some(pen) = self.pen.as_mut() else {
            return Ok(());
        };
        let modifiers = Modifiers { shift, alt: false };
        pen.drag(&mut self.document, Point::new(x, y), modifiers)
            .map_err(to_js)?;
        self.scene_dirty = true;
        Ok(())
    }

    /// End of a pen press. When that completed the path (it closed, or ended
    /// on its last anchor), the path is finished and its id returned.
    #[wasm_bindgen(js_name = penRelease)]
    pub fn pen_release(&mut self) -> Result<Option<String>, JsError> {
        match self.pen.as_mut() {
            Some(pen) if pen.release() => self.pen_finish(),
            _ => Ok(None),
        }
    }

    /// Track the pointer between pen presses, for the preview segment.
    #[wasm_bindgen(js_name = penHover)]
    pub fn pen_hover(&mut self, x: f64, y: f64, tolerance: f64) -> bool {
        match self.pen.as_mut() {
            Some(pen) => {
                pen.hover(Point::new(x, y), tolerance);
                true
            }
            None => false,
        }
    }

    /// Finish the path being drawn. Returns its id, or nothing if there was
    /// no path or it was too short to keep.
    #[wasm_bindgen(js_name = penFinish)]
    pub fn pen_finish(&mut self) -> Result<Option<String>, JsError> {
        let Some(pen) = self.pen.take() else {
            return Ok(None);
        };
        let id = pen
            .finish(&mut self.document, &mut self.journal, &mut self.selection)
            .map_err(to_js)?;
        self.scene_dirty = true;
        Ok(id.map(encode_id))
    }

    /// Leave whichever mode is active: finish the pen path, or stop editing
    /// a path. Returns whether there was one — what Enter and Escape check
    /// before falling back to their other meanings.
    #[wasm_bindgen(js_name = finishMode)]
    pub fn finish_mode(&mut self) -> Result<bool, JsError> {
        if self.pen.is_some() {
            self.pen_finish()?;
            return Ok(true);
        }
        if self.path_edit.is_some() {
            self.end_path_edit()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// "pen" while a path is being drawn, "path" while one is being edited,
    /// nothing otherwise. Cheap, for UI that only needs the mode.
    pub fn mode(&self) -> Option<String> {
        if self.pen.is_some() {
            Some("pen".to_owned())
        } else if self.path_edit.is_some() {
            Some("path".to_owned())
        } else {
            None
        }
    }

    // ---- Path editing ----------------------------------------------------------

    /// Start editing the anchors of the selected path. Returns false unless
    /// exactly one vector node is selected.
    #[wasm_bindgen(js_name = beginPathEdit)]
    pub fn begin_path_edit(&mut self) -> Result<bool, JsError> {
        if self.pen.is_some() {
            return Ok(false);
        }
        let &[id] = self.selection.ids() else {
            return Ok(false);
        };
        self.cancel_gesture()?;
        self.path_edit = PathEdit::begin(&self.document, id).map_err(to_js)?;
        self.hover = None;
        Ok(self.path_edit.is_some())
    }

    /// A press while editing a path: "handle", "anchor", "segment" (an
    /// anchor was inserted there) or "miss".
    #[wasm_bindgen(js_name = pathPress)]
    pub fn path_press(
        &mut self,
        x: f64,
        y: f64,
        tolerance: f64,
        additive: bool,
    ) -> Result<String, JsError> {
        let Some(edit) = self.path_edit.as_mut() else {
            return Ok("miss".to_owned());
        };
        let outcome = edit
            .press(&mut self.document, Point::new(x, y), tolerance, additive)
            .map_err(to_js)?;
        self.scene_dirty = true;
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
        let Some(edit) = self.path_edit.as_mut() else {
            return Ok(());
        };
        edit.update(
            &mut self.document,
            Point::new(x, y),
            Modifiers { shift, alt },
        )
        .map_err(to_js)?;
        self.scene_dirty = true;
        Ok(())
    }

    #[wasm_bindgen(js_name = pathRelease)]
    pub fn path_release(&mut self) -> Result<bool, JsError> {
        let Some(edit) = self.path_edit.as_mut() else {
            return Ok(false);
        };
        let changed = edit
            .release(&mut self.document, &mut self.journal)
            .map_err(to_js)?;
        self.scene_dirty = true;
        Ok(changed)
    }

    #[wasm_bindgen(js_name = pathCancelDrag)]
    pub fn path_cancel_drag(&mut self) -> Result<(), JsError> {
        self.cancel_path_drag()
    }

    /// A double-click while editing: on an anchor it toggles corner/smooth;
    /// on the path it does nothing; off the path it stops editing. Returns
    /// whether editing continues.
    #[wasm_bindgen(js_name = pathDoubleClick)]
    pub fn path_double_click(&mut self, x: f64, y: f64, tolerance: f64) -> Result<bool, JsError> {
        let Some(edit) = self.path_edit.as_mut() else {
            return Ok(false);
        };
        let point = Point::new(x, y);
        let toggled = edit
            .toggle_smooth_at(&mut self.document, &mut self.journal, point, tolerance)
            .map_err(to_js)?;
        if toggled {
            self.scene_dirty = true;
            return Ok(true);
        }
        let id = edit.id();
        if hit::hit_test(&self.document, point, tolerance).map_err(to_js)? == Some(id) {
            return Ok(true);
        }
        self.end_path_edit()?;
        Ok(false)
    }

    /// Everything the selection overlay draws, as JSON, in document units.
    ///
    /// One call per frame, not one per node: frame corners, outline paths of
    /// the selected and hovered nodes, and the marquee. Handles are drawn and
    /// hit-tested by the UI from the frame corners, since their size is a
    /// screen measurement, not a document one.
    pub fn overlay(&self) -> Result<String, JsError> {
        if let Some(pen) = &self.pen {
            return to_json(&self.pen_overlay(pen));
        }
        if let Some(edit) = &self.path_edit {
            let view = edit.view(&self.document).map_err(to_js)?;
            return to_json(&serde_json::json!({
                "mode": "path",
                "frame": null,
                "outlines": [],
                "hover": null,
                "marquee": null,
                "gesture": null,
                "path": {
                    "outline": view.outline.to_svg(),
                    "anchors": view.anchors.iter().map(|(p, selected)| serde_json::json!({
                        "at": [p.x, p.y],
                        "selected": selected,
                    })).collect::<Vec<_>>(),
                    "handles": view.handles.iter().map(|(a, h)| [a.x, a.y, h.x, h.y]).collect::<Vec<_>>(),
                },
            }));
        }

        let frame = match &self.gesture {
            Some(gesture) => match gesture.frame() {
                Some(frame) => Some(frame),
                None => Frame::of(&self.document, self.selection.ids()).map_err(to_js)?,
            },
            None => Frame::of(&self.document, self.selection.ids()).map_err(to_js)?,
        };
        let frame = frame.map(|frame| {
            let (width, height) = frame.size();
            serde_json::json!({
                "corners": frame.corners().map(|p| [p.x, p.y]),
                "width": width,
                "height": height,
            })
        });

        let mut outlines = Vec::with_capacity(self.selection.len());
        for &id in self.selection.ids() {
            if let Some(d) = self.outline(id)? {
                outlines.push(d);
            }
        }
        let hover = match self.hover {
            Some(id) if !self.selection.contains(id) && self.document.is_attached(id) => {
                self.outline(id)?
            }
            _ => None,
        };
        let marquee = self
            .gesture
            .as_ref()
            .and_then(Gesture::marquee_rect)
            .map(|r| [r.x0, r.y0, r.x1, r.y1]);

        to_json(&serde_json::json!({
            "mode": null,
            "frame": frame,
            "outlines": outlines,
            "hover": hover,
            "marquee": marquee,
            "gesture": self.gesture.as_ref().map(Gesture::label),
        }))
    }
}

impl Editor {
    fn begin_transform(&mut self, kind: TransformKind, x: f64, y: f64) -> Result<bool, JsError> {
        self.cancel_gesture()?;
        let gesture = Gesture::transform(&self.document, &self.selection, kind, Point::new(x, y))
            .map_err(to_js)?;
        let started = gesture.is_some();
        self.gesture = gesture;
        Ok(started)
    }

    fn after_history_change(&mut self, changed: bool) -> Result<(), JsError> {
        if !changed {
            return Ok(());
        }
        self.scene_dirty = true;
        self.selection.retain_attached(&self.document);
        if self.hover.is_some_and(|id| !self.document.is_attached(id)) {
            self.hover = None;
        }
        if let Some(edit) = self.path_edit.as_mut() {
            if self.document.is_attached(edit.id()) {
                edit.revalidate(&self.document).map_err(to_js)?;
            } else {
                self.path_edit = None;
            }
        }
        Ok(())
    }

    fn cancel_path_drag(&mut self) -> Result<(), JsError> {
        if let Some(edit) = self.path_edit.as_mut() {
            edit.cancel_drag(&mut self.document).map_err(to_js)?;
            self.scene_dirty = true;
        }
        Ok(())
    }

    fn end_path_edit(&mut self) -> Result<(), JsError> {
        self.cancel_path_drag()?;
        self.path_edit = None;
        Ok(())
    }

    fn pen_overlay(&self, pen: &PenSession) -> serde_json::Value {
        let anchors = pen.anchors();
        // Handles of the anchor being placed, so a drag shows what it pulls.
        let handles: Vec<[f64; 4]> = anchors
            .last()
            .map(|a| {
                [a.handle_in, a.handle_out]
                    .into_iter()
                    .flatten()
                    .map(|h| [a.point.x, a.point.y, h.x, h.y])
                    .collect()
            })
            .unwrap_or_default();
        serde_json::json!({
            "mode": "pen",
            "frame": null,
            "outlines": [],
            "hover": null,
            "marquee": null,
            "gesture": null,
            "pen": {
                "anchors": anchors.iter().map(|a| [a.point.x, a.point.y]).collect::<Vec<_>>(),
                "handles": handles,
                "preview": pen.preview().map(|p| p.to_svg()),
                "closable": pen.closable(),
            },
        })
    }

    /// A node's outline as an SVG path in document space, for the overlay.
    fn outline(&self, id: NodeId) -> Result<Option<String>, JsError> {
        let node = self.document.get(id).map_err(to_js)?;
        let NodeKind::Vector(vector) = &node.kind else {
            return Ok(None);
        };
        let mut path = vector.path.clone();
        path.apply_affine(self.document.world_transform(id).map_err(to_js)?);
        Ok(Some(path.to_svg()))
    }

    fn insert(&mut self, node: Node) -> Result<String, JsError> {
        let parent = self.document.root();
        let index = self.document.children_of(parent).map_err(to_js)?.len();
        self.journal
            .execute(
                &mut self.document,
                Command::InsertNode {
                    parent,
                    index,
                    node,
                },
            )
            .map_err(to_js)?;
        self.scene_dirty = true;
        let id = *self
            .document
            .children_of(parent)
            .map_err(to_js)?
            .last()
            .ok_or_else(|| JsError::new("insert produced no node"))?;
        Ok(encode_id(id))
    }

    fn execute(&mut self, command: Command) -> Result<(), JsError> {
        self.journal
            .execute(&mut self.document, command)
            .map_err(to_js)?;
        self.scene_dirty = true;
        Ok(())
    }
}

fn fill_from_bytes(srgb: &[u8]) -> Result<LinearRgba, JsError> {
    if srgb.len() != 4 {
        return Err(JsError::new("colour must be 4 sRGB bytes"));
    }
    Ok(LinearRgba::from_srgb8(srgb[0], srgb[1], srgb[2], srgb[3]))
}

/// Node ids cross the boundary as opaque strings.
///
/// Encoded from the slotmap key rather than searched for: `setTransform` runs
/// on every frame of a drag, so id lookup has to be O(1). The UI must treat
/// these as opaque and only echo back what it was given.
fn encode_id(id: NodeId) -> String {
    id.data().as_ffi().to_string()
}

fn parse_id(doc: &Document, id: &str) -> Result<NodeId, JsError> {
    let raw: u64 = id.parse().map_err(|_| JsError::new("malformed node id"))?;
    let id = NodeId::from(KeyData::from_ffi(raw));
    if !doc.contains(id) {
        return Err(JsError::new("unknown node id"));
    }
    Ok(id)
}

fn to_json(value: &serde_json::Value) -> Result<String, JsError> {
    serde_json::to_string(value).map_err(|e| JsError::new(&e.to_string()))
}

fn to_js(e: graphicgene_core::error::CoreError) -> JsError {
    JsError::new(&e.to_string())
}
