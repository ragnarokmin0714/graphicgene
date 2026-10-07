//! The node tree.
//!
//! `NodeKind` is an enum rather than a trait object: `dyn` hurts both
//! serialization and hot-path performance, and adding `Raster` / `Component` /
//! `Text` later is a new variant plus match arms the compiler will point us at.

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::color::LinearRgba;
use crate::geom::{Affine, BezPath, Bounds, Shape};
use crate::paint::Paint;
use crate::text::TextNode;

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

/// How a stroke's open ends are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineCap {
    /// Square, ending at the end point.
    #[default]
    Butt,
    Round,
    /// Square, reaching half the width past the end point.
    Square,
}

/// How a stroke turns a corner.
///
/// A miter is cut off where it would reach past four half-widths — the
/// limit tiny-skia and SVG both use by default — which is what the
/// renderer's damage rects allow for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// A dashed stroke: `length` of line, then `gap`, repeating from the start
/// of the path. In document units, not multiples of the width.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Dash {
    pub length: f64,
    pub gap: f64,
}

/// A path's outline paint. The style fields are left out of the file when
/// they hold their defaults, so a plain stroke reads as it always did.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: LinearRgba,
    pub width: f64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub cap: LineCap,
    #[serde(default, skip_serializing_if = "is_default")]
    pub join: LineJoin,
    /// None for a solid line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash: Option<Dash>,
}

impl Stroke {
    /// A solid stroke with butt caps and miter joins.
    pub const fn solid(color: LinearRgba, width: f64) -> Self {
        Self {
            color,
            width,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            dash: None,
        }
    }
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorNode {
    pub path: BezPath,
    pub fill: Option<Paint>,
    pub stroke: Option<Stroke>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupNode {
    pub children: Vec<NodeId>,
}

/// The kinds of node a document can hold.
///
/// Later: `Raster` (pixel buffers), `Component` (reference + overrides).
/// Neither exists yet — see the scope rule in CLAUDE.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeKind {
    Group(GroupNode),
    Vector(VectorNode),
    Text(TextNode),
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

    /// A path filled with one colour, or not filled; no stroke.
    pub fn vector(name: impl Into<String>, path: BezPath, fill: Option<LinearRgba>) -> Self {
        Self {
            common: NodeCommon {
                name: name.into(),
                ..Default::default()
            },
            kind: NodeKind::Vector(VectorNode {
                path,
                fill: fill.map(Paint::Solid),
                stroke: None,
            }),
        }
    }

    pub fn text(name: impl Into<String>, text: TextNode) -> Self {
        Self {
            common: NodeCommon {
                name: name.into(),
                ..Default::default()
            },
            kind: NodeKind::Text(text),
        }
    }

    pub fn children(&self) -> Option<&[NodeId]> {
        match &self.kind {
            NodeKind::Group(g) => Some(&g.children),
            NodeKind::Vector(_) | NodeKind::Text(_) => None,
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<NodeId>> {
        match &mut self.kind {
            NodeKind::Group(g) => Some(&mut g.children),
            NodeKind::Vector(_) | NodeKind::Text(_) => None,
        }
    }

    /// Bounds in this node's own coordinate space, before `transform`. For
    /// text, the text box.
    pub fn local_bounds(&self) -> Option<Bounds> {
        match &self.kind {
            NodeKind::Vector(v) => Some(v.path.bounding_box()),
            NodeKind::Text(t) => Some(t.bounds()),
            NodeKind::Group(_) => None,
        }
    }
}
