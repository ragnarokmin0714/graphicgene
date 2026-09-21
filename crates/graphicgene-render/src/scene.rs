//! The render scene: a flat, immutable snapshot of what to draw.
//!
//! The renderer never walks the live document. Editing and drawing are
//! separate stages so that the document can be mutated freely without the
//! renderer being exposed to partially-applied state, and so the snapshot can
//! later be handed to another thread or a GPU backend unchanged.
//!
//! The scene is rebuilt per edit, not per frame.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::doc::Document;
use graphicgene_core::error::Result;
use graphicgene_core::geom::{Affine, BezPath, Bounds, Shape};
use graphicgene_core::node::{BlendMode, NodeKind, Stroke};

/// One drawable, with its world transform and accumulated opacity baked in.
#[derive(Debug, Clone)]
pub struct RenderItem {
    pub transform: Affine,
    pub path: BezPath,
    pub fill: Option<LinearRgba>,
    pub stroke: Option<Stroke>,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    /// World-space bounds, used for dirty-rect culling and hit-testing.
    pub bounds: Bounds,
}

#[derive(Debug, Clone, Default)]
pub struct RenderScene {
    pub items: Vec<RenderItem>,
}

impl RenderScene {
    /// Flatten a document into a draw list, back to front.
    pub fn build(doc: &Document) -> Result<Self> {
        let mut items = Vec::new();
        visit(doc, doc.root(), Affine::IDENTITY, 1.0, &mut items)?;
        Ok(Self { items })
    }

    pub fn bounds(&self) -> Option<Bounds> {
        self.items
            .iter()
            .map(|i| i.bounds)
            .reduce(|a, b| a.union(b))
    }
}

fn visit(
    doc: &Document,
    id: graphicgene_core::node::NodeId,
    parent_transform: Affine,
    parent_opacity: f32,
    out: &mut Vec<RenderItem>,
) -> Result<()> {
    let node = doc.get(id)?;
    if !node.common.visible || node.common.opacity <= 0.0 {
        return Ok(());
    }

    let transform = parent_transform * node.common.transform;
    let opacity = parent_opacity * node.common.opacity;

    if let NodeKind::Vector(v) = &node.kind {
        let bounds = transform.transform_rect_bbox(v.path.bounding_box());
        out.push(RenderItem {
            transform,
            path: v.path.clone(),
            fill: v.fill,
            stroke: v.stroke,
            opacity,
            blend_mode: node.common.blend_mode,
            bounds,
        });
    }

    for child in doc.children_of(id)? {
        visit(doc, *child, transform, opacity, out)?;
    }
    Ok(())
}
