//! The node tree.
//!
//! `NodeKind` is an enum rather than a trait object: `dyn` hurts both
//! serialization and hot-path performance, and adding `Raster` / `Component` /
//! `Text` later is a new variant plus match arms the compiler will point us at.

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::color::LinearRgba;
use crate::geom::{Affine, BezPath, Bounds, Shape};

new_key_type! {
    /// Stable identity for a node, preserved across save/load.
    ///
    /// Components (referencing another node's subtree) and any future
    /// collaboration model both depend on this being stable.
    pub struct NodeId;
}

/// How a node composites onto what is below it.
///
/// Only `Normal` is implemented in v0.1. The field exists from day one because
/// it changes the shape of the render pipeline — retrofitting it means
/// rewriting that pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
}

/// Fields every node has, whatever its kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCommon {
    pub name: String,
    pub transform: Affine,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    /// Whether this node clips its descendants to its own geometry.
    pub clip: bool,
    pub visible: bool,
    pub locked: bool,
    pub parent: Option<NodeId>,
}

impl Default for NodeCommon {
    fn default() -> Self {
        Self {
            name: String::new(),
            transform: Affine::IDENTITY,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            clip: false,
            visible: true,
            locked: false,
            parent: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: LinearRgba,
    pub width: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorNode {
    pub path: BezPath,
    pub fill: Option<LinearRgba>,
    pub stroke: Option<Stroke>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupNode {
    pub children: Vec<NodeId>,
}

/// The kinds of node a document can hold.
///
/// Later: `Raster` (pixel buffers), `Component` (reference + overrides),
/// `Text` (shaped runs). None of them exist yet — see the scope rule in
/// CLAUDE.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeKind {
    Group(GroupNode),
    Vector(VectorNode),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    #[serde(flatten)]
    pub common: NodeCommon,
    pub kind: NodeKind,
}

impl Node {
    pub fn group(name: impl Into<String>) -> Self {
        Self {
            common: NodeCommon {
                name: name.into(),
                ..Default::default()
            },
            kind: NodeKind::Group(GroupNode::default()),
        }
    }

    pub fn vector(name: impl Into<String>, path: BezPath, fill: Option<LinearRgba>) -> Self {
        Self {
            common: NodeCommon {
                name: name.into(),
                ..Default::default()
            },
            kind: NodeKind::Vector(VectorNode {
                path,
                fill,
                stroke: None,
            }),
        }
    }

    pub fn children(&self) -> Option<&[NodeId]> {
        match &self.kind {
            NodeKind::Group(g) => Some(&g.children),
            NodeKind::Vector(_) => None,
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<NodeId>> {
        match &mut self.kind {
            NodeKind::Group(g) => Some(&mut g.children),
            NodeKind::Vector(_) => None,
        }
    }

    /// Bounds in this node's own coordinate space, before `transform`.
    pub fn local_bounds(&self) -> Option<Bounds> {
        match &self.kind {
            NodeKind::Vector(v) => Some(v.path.bounding_box()),
            NodeKind::Group(_) => None,
        }
    }
}
