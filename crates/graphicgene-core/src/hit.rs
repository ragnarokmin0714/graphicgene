//! Hit-testing: which node is under a point, which nodes a marquee touches.
//!
//! v0.1 scans linearly. Nothing in these signatures assumes that, so a spatial
//! index can replace the scan without the callers changing.

use kurbo::ParamCurveNearest;

use crate::doc::Document;
use crate::error::Result;
use crate::geom::{Affine, Bounds, Point, Shape, is_empty_bounds};
use crate::node::{NodeId, NodeKind, VectorNode};

/// The topmost selectable node under `point`, in document space.
///
/// Hidden and locked nodes are skipped, along with everything inside them.
/// What comes back is the node's top-level ancestor, the one a click should
/// select: clicking a shape inside a group picks the group, as in Figma.
///
/// `tolerance` is how far outside a shape's edge still counts, in document
/// units. Unfilled paths are hit only along their outline.
pub fn hit_test(doc: &Document, point: Point, tolerance: f64) -> Result<Option<NodeId>> {
    match topmost(doc, doc.root(), Affine::IDENTITY, point, tolerance)? {
        Some(id) => Ok(Some(top_level(doc, id)?)),
        None => Ok(None),
    }
}

/// Top-level selectable nodes whose bounds overlap `rect`, in paint order.
///
/// Overlap rather than containment, matching Figma: a marquee only has to
/// touch a node to pick it up.
pub fn nodes_in_rect(doc: &Document, rect: Bounds) -> Result<Vec<NodeId>> {
    let mut out = Vec::new();
    for &id in doc.children_of(doc.root())? {
        let node = doc.get(id)?;
        if !node.common.visible || node.common.locked {
            continue;
        }
        let bounds = doc.world_bounds(id)?;
        if !is_empty_bounds(bounds) && overlaps(bounds, rect) {
            out.push(id);
        }
    }
    Ok(out)
}

/// Every top-level selectable node, in paint order. What Select All picks.
pub fn selectable(doc: &Document) -> Result<Vec<NodeId>> {
    let mut out = Vec::new();
    for &id in doc.children_of(doc.root())? {
        let node = doc.get(id)?;
        if node.common.visible && !node.common.locked {
            out.push(id);
        }
    }
    Ok(out)
}

/// Reverse paint order without collecting a list first: children are painted
/// after (above) their parent, and later siblings above earlier ones, so the
/// last child's subtree is searched first and the node itself last.
fn topmost(
    doc: &Document,
    id: NodeId,
    parent: Affine,
    point: Point,
    tolerance: f64,
) -> Result<Option<NodeId>> {
    let node = doc.get(id)?;
    if !node.common.visible || node.common.locked {
        return Ok(None);
    }
    let world = parent * node.common.transform;
    for &child in doc.children_of(id)?.iter().rev() {
        if let Some(hit) = topmost(doc, child, world, point, tolerance)? {
            return Ok(Some(hit));
        }
    }
    if let NodeKind::Vector(vector) = &node.kind
        && vector_hit(vector, world, point, tolerance)
    {
        return Ok(Some(id));
    }
    Ok(None)
}

fn vector_hit(vector: &VectorNode, world: Affine, point: Point, tolerance: f64) -> bool {
    let det = world.determinant();
    if det.abs() < 1e-12 {
        return false;
    }
    let local = world.inverse() * point;
    // Tolerance is given in document units; convert it into this node's local
    // units with the transform's average scale.
    let reach = tolerance / det.abs().sqrt() + vector.stroke.map_or(0.0, |s| s.width / 2.0);
    // Broad phase. A bezier never leaves the hull of its control points, so
    // a point outside the control box, grown by the reach, can neither be
    // inside the fill nor within reach of the outline. This skips the root
    // solving below for almost every node a hover passes over.
    let near = vector.path.control_box().inflate(reach, reach);
    if !(near.x0 <= local.x && local.x <= near.x1 && near.y0 <= local.y && local.y <= near.y1) {
        return false;
    }
    if vector.fill.is_some() && vector.path.winding(local) != 0 {
        return true;
    }
    vector
        .path
        .segments()
        .any(|segment| segment.nearest(local, 1e-3).distance_sq <= reach * reach)
}

fn top_level(doc: &Document, mut id: NodeId) -> Result<NodeId> {
    let root = doc.root();
    while let Some(parent) = doc.get(id)?.common.parent {
        if parent == root {
            break;
        }
        id = parent;
    }
    Ok(id)
}

fn overlaps(a: Bounds, b: Bounds) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}
