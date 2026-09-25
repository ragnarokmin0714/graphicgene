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
use graphicgene_core::geom::{Affine, BezPath, Point, Rect, Shape};
use graphicgene_core::gesture::{self, Frame, Gesture, Modifiers, ShapeKind, TransformKind};
use graphicgene_core::hit;
use graphicgene_core::node::{Node, NodeId, NodeKind};
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
}

/// How far outside a shape a click still hits it, in document units.
/// When zoom arrives this becomes a screen distance divided by the zoom.
const HIT_TOLERANCE: f64 = 4.0;

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

    pub fn undo(&mut self) -> Result<bool, JsError> {
        self.cancel_gesture()?;
        let changed = self.journal.undo(&mut self.document).map_err(to_js)?;
        self.after_history_change(changed);
        Ok(changed)
    }

    pub fn redo(&mut self) -> Result<bool, JsError> {
        self.cancel_gesture()?;
        let changed = self.journal.redo(&mut self.document).map_err(to_js)?;
        self.after_history_change(changed);
        Ok(changed)
    }

    #[wasm_bindgen(js_name = canUndo)]
    pub fn can_undo(&self) -> bool {
        self.journal.can_undo()
    }

    #[wasm_bindgen(js_name = canRedo)]
    pub fn can_redo(&self) -> bool {
        self.journal.can_redo()
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
    #[wasm_bindgen(js_name = toJson)]
    pub fn to_json(&self) -> Result<String, JsError> {
        Project::new(self.document.clone()).to_json().map_err(to_js)
    }

    #[wasm_bindgen(js_name = loadJson)]
    pub fn load_json(&mut self, text: &str) -> Result<(), JsError> {
        let project = Project::from_json(text).map_err(to_js)?;
        self.gesture = None;
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
        if additive {
            self.selection.toggle(id);
        } else {
            self.selection.set([id]);
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = selectAll)]
    pub fn select_all(&mut self) -> Result<(), JsError> {
        let ids = hit::selectable(&self.document).map_err(to_js)?;
        self.selection.set(ids);
        Ok(())
    }

    #[wasm_bindgen(js_name = clearSelection)]
    pub fn clear_selection(&mut self) {
        self.selection.clear();
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

    /// Remove the selected nodes, as one undo step.
    #[wasm_bindgen(js_name = deleteSelection)]
    pub fn delete_selection(&mut self) -> Result<bool, JsError> {
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
        if self.selection.is_empty() || self.gesture.is_some() {
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

    /// Everything the selection overlay draws, as JSON, in document units.
    ///
    /// One call per frame, not one per node: frame corners, outline paths of
    /// the selected and hovered nodes, and the marquee. Handles are drawn and
    /// hit-tested by the UI from the frame corners, since their size is a
    /// screen measurement, not a document one.
    pub fn overlay(&self) -> Result<String, JsError> {
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

        let json = serde_json::json!({
            "frame": frame,
            "outlines": outlines,
            "hover": hover,
            "marquee": marquee,
            "gesture": self.gesture.as_ref().map(Gesture::label),
        });
        serde_json::to_string(&json).map_err(|e| JsError::new(&e.to_string()))
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

    fn after_history_change(&mut self, changed: bool) {
        if changed {
            self.scene_dirty = true;
            self.selection.retain_attached(&self.document);
            if self.hover.is_some_and(|id| !self.document.is_attached(id)) {
                self.hover = None;
            }
        }
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

fn to_js(e: graphicgene_core::error::CoreError) -> JsError {
    JsError::new(&e.to_string())
}
