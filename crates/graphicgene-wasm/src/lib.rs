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
use graphicgene_core::geom::{Affine, BezPath, Rect, Shape};
use graphicgene_core::node::{Node, NodeId};
use graphicgene_core::project::Project;
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
}

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
        let changed = self.journal.undo(&mut self.document).map_err(to_js)?;
        self.scene_dirty |= changed;
        Ok(changed)
    }

    pub fn redo(&mut self) -> Result<bool, JsError> {
        let changed = self.journal.redo(&mut self.document).map_err(to_js)?;
        self.scene_dirty |= changed;
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
        self.document = project.document;
        self.journal.clear();
        self.scene_dirty = true;
        Ok(())
    }

    /// The layer tree, as JSON, for the UI to render.
    #[wasm_bindgen(js_name = layerTree)]
    pub fn layer_tree(&self) -> Result<String, JsError> {
        let ids = self.document.walk();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let node = self.document.get(id).map_err(to_js)?;
            out.push(serde_json::json!({
                "id": encode_id(id),
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
}

impl Editor {
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
