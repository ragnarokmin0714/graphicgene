//! The render scene: a flat snapshot of what to draw.
//!
//! The renderer never walks the live document. Editing and drawing are
//! separate stages so that the document can be mutated freely without the
//! renderer being exposed to partially-applied state, and so the snapshot can
//! later be handed to another thread or a GPU backend unchanged.
//!
//! The scene is kept up to date from the document's change log rather than
//! rebuilt: `update` refreshes only the items of nodes that changed, and
//! reports the area that needs redrawing — where those items were, and where
//! they are now. Only a change to the tree's shape rebuilds it.

use std::collections::HashMap;

use graphicgene_core::color::LinearRgba;
use graphicgene_core::doc::{Changes, Document};
use graphicgene_core::error::Result;
use graphicgene_core::geom::{
    Affine, BezPath, Bounds, Rect, Shape, empty_bounds, is_empty_bounds, union,
};
use graphicgene_core::node::{BlendMode, NodeId, NodeKind, Stroke, VectorNode};

/// Antialiasing touches up to a pixel beyond a shape's edge — a *device*
/// pixel, whatever the zoom, which is why it is added after the view
/// transform (`device_area`) and not to the document-space bounds.
pub const AA_MARGIN: f64 = 1.0;
/// How far a stroke can reach past its path, in stroke widths: half the
/// width, times the renderer's miter limit of 4 for sharp corners.
const STROKE_REACH: f64 = 2.0;

/// The device pixels a document-space area can touch once drawn through
/// `view` (document to device): what to cull against and what to redraw.
pub fn device_area(view: Affine, area: Bounds) -> Bounds {
    view.transform_rect_bbox(area).inflate(AA_MARGIN, AA_MARGIN)
}

/// One drawable, with its world transform and accumulated opacity baked in.
#[derive(Debug, Clone)]
pub struct RenderItem {
    pub node: NodeId,
    pub transform: Affine,
    pub path: BezPath,
    pub fill: Option<LinearRgba>,
    pub stroke: Option<Stroke>,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    /// Document-space bounds of everything this item draws, stroke
    /// included; `device_area` adds the antialiasing margin.
    pub bounds: Bounds,
}

/// The page the artwork sits on, drawn under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Artboard {
    pub rect: Rect,
    pub fill: LinearRgba,
}

#[derive(Debug, Clone, Default)]
pub struct RenderScene {
    /// Back to front.
    pub items: Vec<RenderItem>,
    /// What the canvas is cleared to before drawing — the backdrop around
    /// the artboard. `None` is transparent.
    pub background: Option<LinearRgba>,
    /// Taken from the document on every rebuild.
    pub artboard: Option<Artboard>,
    /// Where each vector node's item is in `items`.
    index: HashMap<NodeId, usize>,
}

/// The part of the canvas an update invalidated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Damage {
    /// Nothing on screen changed.
    None,
    /// Only this document-space area needs redrawing; map it with
    /// `device_area`.
    Region(Bounds),
    /// Redraw everything.
    Everything,
}

impl RenderScene {
    /// Flatten a document into a draw list, back to front.
    pub fn build(doc: &Document) -> Result<Self> {
        let mut scene = Self::default();
        scene.rebuild(doc)?;
        Ok(scene)
    }

    /// Bring the scene up to date with `changes` and report what to redraw.
    ///
    /// Items are refreshed in place for each changed node and its subtree.
    /// A change to the tree's shape — or a node appearing or disappearing —
    /// changes the item list itself, so the scene is rebuilt and everything
    /// redrawn; those are single events, never the per-frame path.
    pub fn update(&mut self, doc: &Document, changes: &Changes) -> Result<Damage> {
        if changes.everything || changes.structure {
            self.rebuild(doc)?;
            return Ok(Damage::Everything);
        }
        let mut region = empty_bounds();
        for &id in &changes.nodes {
            if !doc.contains(id) || !doc.is_attached(id) {
                continue;
            }
            let (transform, opacity, visible) = ancestry(doc, id)?;
            if !self.refresh(doc, id, transform, opacity, visible, &mut region)? {
                self.rebuild(doc)?;
                return Ok(Damage::Everything);
            }
        }
        Ok(if is_empty_bounds(region) {
            Damage::None
        } else {
            Damage::Region(region)
        })
    }

    pub fn bounds(&self) -> Option<Bounds> {
        self.items
            .iter()
            .map(|i| i.bounds)
            .reduce(|a, b| a.union(b))
    }

    fn rebuild(&mut self, doc: &Document) -> Result<()> {
        self.items.clear();
        self.index.clear();
        let size = doc.artboard();
        self.artboard = Some(Artboard {
            rect: Rect::new(0.0, 0.0, size.width, size.height),
            fill: LinearRgba::WHITE,
        });
        self.visit(doc, doc.root(), Affine::IDENTITY, 1.0)
    }

    fn visit(
        &mut self,
        doc: &Document,
        id: NodeId,
        parent_transform: Affine,
        parent_opacity: f32,
    ) -> Result<()> {
        let node = doc.get(id)?;
        if !node.common.visible || node.common.opacity <= 0.0 {
            return Ok(());
        }
        let transform = parent_transform * node.common.transform;
        let opacity = parent_opacity * node.common.opacity;

        if let NodeKind::Vector(vector) = &node.kind {
            self.index.insert(id, self.items.len());
            self.items
                .push(item(id, vector, transform, opacity, node.common.blend_mode));
        }
        for &child in doc.children_of(id)? {
            self.visit(doc, child, transform, opacity)?;
        }
        Ok(())
    }

    /// Recompute the items of `id`'s subtree, growing `region` by where each
    /// one was and now is. Returns false if an item would have to appear or
    /// disappear, which only a rebuild can do.
    fn refresh(
        &mut self,
        doc: &Document,
        id: NodeId,
        parent_transform: Affine,
        parent_opacity: f32,
        parent_visible: bool,
        region: &mut Bounds,
    ) -> Result<bool> {
        let node = doc.get(id)?;
        // The same test `visit` applies, carried down the subtree.
        let visible = parent_visible && node.common.visible && node.common.opacity > 0.0;
        let transform = parent_transform * node.common.transform;
        let opacity = parent_opacity * node.common.opacity;

        if let NodeKind::Vector(vector) = &node.kind {
            match (visible, self.index.get(&id)) {
                (true, Some(&i)) => {
                    let fresh = item(id, vector, transform, opacity, node.common.blend_mode);
                    *region = union(union(*region, self.items[i].bounds), fresh.bounds);
                    self.items[i] = fresh;
                }
                (false, None) => {}
                _ => return Ok(false),
            }
        }
        for &child in doc.children_of(id)? {
            if !self.refresh(doc, child, transform, opacity, visible, region)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// The accumulated transform and opacity above `id`, and whether every
/// ancestor is drawn at all — the state `visit` would arrive at `id` with.
fn ancestry(doc: &Document, id: NodeId) -> Result<(Affine, f32, bool)> {
    let mut chain = Vec::new();
    let mut cursor = doc.get(id)?.common.parent;
    while let Some(ancestor) = cursor {
        chain.push(ancestor);
        cursor = doc.get(ancestor)?.common.parent;
    }
    let (mut transform, mut opacity, mut visible) = (Affine::IDENTITY, 1.0f32, true);
    for &ancestor in chain.iter().rev() {
        let common = &doc.get(ancestor)?.common;
        transform *= common.transform;
        opacity *= common.opacity;
        visible &= common.visible && common.opacity > 0.0;
    }
    Ok((transform, opacity, visible))
}

fn item(
    node: NodeId,
    vector: &VectorNode,
    transform: Affine,
    opacity: f32,
    blend_mode: BlendMode,
) -> RenderItem {
    let reach = vector.stroke.map_or(0.0, |s| s.width * STROKE_REACH);
    let local = vector.path.bounding_box().inflate(reach, reach);
    let bounds = transform.transform_rect_bbox(local);
    RenderItem {
        node,
        transform,
        path: vector.path.clone(),
        fill: vector.fill,
        stroke: vector.stroke,
        opacity,
        blend_mode,
        bounds,
    }
}
