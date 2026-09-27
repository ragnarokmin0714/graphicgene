//! The properties panel's view of the selection, and edits made through it.
//!
//! Reading: what the selected nodes have in common — the frame's position,
//! size and rotation, opacity, fill and stroke — with `Shared::Mixed` where
//! they differ.
//!
//! Writing: a property edit previews straight into the document and commits
//! as one journal entry, the same shape as a canvas drag. A slider dragged
//! through fifty values, or a number scrubbed across a hundred, is one undo
//! step. Every preview starts again from the state before the edit, so a
//! value dragged back to where it began leaves nothing to undo.
//!
//! Rotation reads counter-clockwise, as in Figma and Illustrator; the
//! document's y axis points down, so that is the negated maths angle.

use crate::color::LinearRgba;
use crate::command::{Command, Journal};
use crate::doc::Document;
use crate::error::Result;
use crate::geom::{Affine, Vec2};
use crate::gesture::Frame;
use crate::node::{Node, NodeId, NodeKind, Stroke, VectorNode};

/// The smallest width or height a field can set, in document units: a frame
/// scaled to nothing could never be scaled back.
const MIN_EXTENT: f64 = 0.01;
/// A stroke added by picking a colour gets this width.
const DEFAULT_STROKE_WIDTH: f64 = 1.0;

/// A value every selected node shares, or not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shared<T> {
    Same(T),
    Mixed,
}

impl<T: PartialEq + Copy> Shared<T> {
    /// `None` when there are no values at all.
    fn of(mut values: impl Iterator<Item = T>) -> Option<Self> {
        let first = values.next()?;
        Some(if values.all(|v| v == first) {
            Shared::Same(first)
        } else {
            Shared::Mixed
        })
    }
}

/// What the properties panel shows for the selection.
#[derive(Debug, Clone, PartialEq)]
pub struct Properties {
    pub count: usize,
    /// The selection frame's top-left corner, in the frame's own
    /// orientation, in document units.
    pub x: f64,
    pub y: f64,
    /// The frame's edge lengths.
    pub width: f64,
    pub height: f64,
    /// Degrees, counter-clockwise, in (-180, 180]. Always 0 for several
    /// nodes, whose shared frame is axis-aligned.
    pub rotation: f64,
    pub opacity: Shared<f32>,
    /// The fill colour, `Same(None)` for no fill. `None` when nothing
    /// selected has paint: groups only.
    pub fill: Option<Shared<Option<LinearRgba>>>,
    /// The stroke colour, `Same(None)` for no stroke, and `None` for groups
    /// only, as for `fill`.
    pub stroke: Option<Shared<Option<LinearRgba>>>,
    /// The width of the strokes there are. `None` when nothing selected has
    /// a stroke.
    pub stroke_width: Option<Shared<f64>>,
}

/// One change the panel can make.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Property {
    X(f64),
    Y(f64),
    Width(f64),
    Height(f64),
    /// Degrees, counter-clockwise.
    Rotation(f64),
    Opacity(f32),
    Fill(Option<LinearRgba>),
    /// Set or remove the whole stroke.
    Stroke(Option<Stroke>),
    /// Recolour strokes, keeping each one's width; paths without a stroke
    /// get a thin one.
    StrokeColor(LinearRgba),
    /// Re-width strokes, keeping each one's colour; paths without a stroke
    /// are left alone.
    StrokeWidth(f64),
}

pub fn properties(doc: &Document, ids: &[NodeId]) -> Result<Option<Properties>> {
    let Some(frame) = Frame::of(doc, ids)? else {
        return Ok(None);
    };
    let [top_left, ..] = frame.corners();
    let (width, height) = frame.size();
    let rotation = if ids.len() == 1 {
        counter_clockwise_degrees(frame.angle())
    } else {
        0.0
    };
    let nodes = ids
        .iter()
        .map(|&id| doc.get(id))
        .collect::<Result<Vec<&Node>>>()?;
    let vectors: Vec<&VectorNode> = nodes
        .iter()
        .filter_map(|node| match &node.kind {
            NodeKind::Vector(vector) => Some(vector),
            NodeKind::Group(_) => None,
        })
        .collect();
    Ok(Some(Properties {
        count: ids.len(),
        x: top_left.x,
        y: top_left.y,
        width,
        height,
        rotation,
        opacity: Shared::of(nodes.iter().map(|n| n.common.opacity)).unwrap_or(Shared::Same(1.0)),
        fill: Shared::of(vectors.iter().map(|v| v.fill)),
        stroke: Shared::of(vectors.iter().map(|v| v.stroke.map(|s| s.color))),
        stroke_width: Shared::of(vectors.iter().filter_map(|v| v.stroke).map(|s| s.width)),
    }))
}

/// What one selected node looked like when the edit began.
#[derive(Debug, Clone, Copy)]
struct Snapshot {
    id: NodeId,
    transform: Affine,
    opacity: f32,
    /// Fill and stroke, for vector nodes.
    paint: Option<(Option<LinearRgba>, Option<Stroke>)>,
}

/// An edit through the properties panel, from the first preview to its
/// commit or cancel.
#[derive(Debug, Clone)]
pub struct PropertyEdit {
    base: Vec<Snapshot>,
}

impl PropertyEdit {
    pub fn begin(doc: &Document, ids: &[NodeId]) -> Result<Self> {
        let mut base = Vec::with_capacity(ids.len());
        for &id in ids {
            let node = doc.get(id)?;
            base.push(Snapshot {
                id,
                transform: node.common.transform,
                opacity: node.common.opacity,
                paint: match &node.kind {
                    NodeKind::Vector(v) => Some((v.fill, v.stroke)),
                    NodeKind::Group(_) => None,
                },
            });
        }
        Ok(Self { base })
    }

    /// Show `property` applied to the nodes as they were when the edit began.
    pub fn preview(&self, doc: &mut Document, property: Property) -> Result<()> {
        self.restore(doc)?;
        let ids: Vec<NodeId> = self.base.iter().map(|s| s.id).collect();
        apply(doc, &ids, property)
    }

    /// Record what the previews left as one journal entry. Returns false,
    /// recording nothing, if the nodes ended where they began.
    pub fn commit(self, doc: &mut Document, journal: &mut Journal) -> Result<bool> {
        let mut commands = Vec::new();
        for snapshot in &self.base {
            let node = doc.get(snapshot.id)?;
            let id = snapshot.id;
            if node.common.transform != snapshot.transform {
                let transform = node.common.transform;
                commands.push(Command::SetTransform { id, transform });
            }
            if node.common.opacity != snapshot.opacity {
                let opacity = node.common.opacity;
                commands.push(Command::SetOpacity { id, opacity });
            }
            if let (NodeKind::Vector(v), Some((fill, stroke))) = (&node.kind, snapshot.paint) {
                if v.fill != fill {
                    commands.push(Command::SetFill { id, fill: v.fill });
                }
                if v.stroke != stroke {
                    commands.push(Command::SetStroke {
                        id,
                        stroke: v.stroke,
                    });
                }
            }
        }
        self.restore(doc)?;
        if commands.is_empty() {
            return Ok(false);
        }
        journal.execute(doc, Command::Batch(commands))?;
        Ok(true)
    }

    /// Put everything back as it was before the edit.
    pub fn cancel(self, doc: &mut Document) -> Result<()> {
        self.restore(doc)
    }

    fn restore(&self, doc: &mut Document) -> Result<()> {
        for snapshot in &self.base {
            let node = doc.get_mut(snapshot.id)?;
            node.common.transform = snapshot.transform;
            node.common.opacity = snapshot.opacity;
            if let (NodeKind::Vector(v), Some((fill, stroke))) = (&mut node.kind, snapshot.paint) {
                v.fill = fill;
                v.stroke = stroke;
            }
        }
        Ok(())
    }
}

fn apply(doc: &mut Document, ids: &[NodeId], property: Property) -> Result<()> {
    match property {
        Property::X(_)
        | Property::Y(_)
        | Property::Width(_)
        | Property::Height(_)
        | Property::Rotation(_) => {
            let Some(frame) = Frame::of(doc, ids)? else {
                return Ok(());
            };
            if let Some(delta) = frame_delta(&frame, ids.len(), property) {
                move_nodes(doc, ids, delta)?;
            }
        }
        Property::Opacity(opacity) => {
            let opacity = if opacity.is_finite() {
                opacity.clamp(0.0, 1.0)
            } else {
                1.0
            };
            for &id in ids {
                doc.get_mut(id)?.common.opacity = opacity;
            }
        }
        Property::Fill(fill) => paint(doc, ids, |v| v.fill = fill)?,
        Property::Stroke(stroke) => paint(doc, ids, |v| v.stroke = stroke)?,
        Property::StrokeColor(color) => paint(doc, ids, |v| {
            let width = v.stroke.map_or(DEFAULT_STROKE_WIDTH, |s| s.width);
            v.stroke = Some(Stroke { color, width });
        })?,
        Property::StrokeWidth(width) => {
            let width = if width.is_finite() {
                width.max(0.0)
            } else {
                DEFAULT_STROKE_WIDTH
            };
            paint(doc, ids, |v| {
                if let Some(stroke) = &mut v.stroke {
                    stroke.width = width;
                }
            })?;
        }
    }
    Ok(())
}

/// The document-space change a position, size or rotation field asks for,
/// or `None` for no change. No change has to be caught here: an identity
/// composed through a rotated parent comes back a few ulps off, and would
/// record an undo step for retyping a value.
fn frame_delta(frame: &Frame, count: usize, property: Property) -> Option<Affine> {
    let (Property::X(value)
    | Property::Y(value)
    | Property::Width(value)
    | Property::Height(value)
    | Property::Rotation(value)) = property
    else {
        return None;
    };
    if !value.is_finite() {
        return None;
    }
    let [top_left, ..] = frame.corners();
    let (width, height) = frame.size();
    let delta = match property {
        Property::X(x) => Affine::translate(Vec2::new(x - top_left.x, 0.0)),
        Property::Y(y) => Affine::translate(Vec2::new(0.0, y - top_left.y)),
        Property::Width(w) => scale_from_corner(frame, stretch(w, width)?, 1.0)?,
        Property::Height(h) => scale_from_corner(frame, 1.0, stretch(h, height)?)?,
        Property::Rotation(degrees) => {
            let current = if count == 1 {
                counter_clockwise_degrees(frame.angle())
            } else {
                0.0
            };
            // The short way round, so 360° on an unrotated node is no turn.
            let turn = (degrees - current).rem_euclid(360.0);
            let turn = if turn > 180.0 { turn - 360.0 } else { turn };
            // Counter-clockwise on screen is the negative maths angle, as y
            // points down.
            Affine::rotate_about(-turn.to_radians(), frame.point_at(0.5, 0.5))
        }
        _ => return None,
    };
    (delta != Affine::IDENTITY).then_some(delta)
}

/// The factor that takes an edge of length `old` to `new`, if it has any
/// length to scale.
fn stretch(new: f64, old: f64) -> Option<f64> {
    (old >= 1e-9).then(|| new.max(MIN_EXTENT) / old)
}

/// Scale in the frame's own space about its top-left corner, so a rotated
/// node keeps its corner and its angle. `None` when neither factor changes
/// anything.
fn scale_from_corner(frame: &Frame, sx: f64, sy: f64) -> Option<Affine> {
    if sx == 1.0 && sy == 1.0 {
        return None;
    }
    let corner = Vec2::new(frame.rect.x0, frame.rect.y0);
    let scale =
        Affine::translate(corner) * Affine::scale_non_uniform(sx, sy) * Affine::translate(-corner);
    Some(frame.transform * scale * frame.transform.inverse())
}

/// Apply a document-space change to each node through its parent.
fn move_nodes(doc: &mut Document, ids: &[NodeId], delta: Affine) -> Result<()> {
    for &id in ids {
        let parent = doc.parent_world_transform(id)?;
        let node = doc.get_mut(id)?;
        node.common.transform = parent.inverse() * delta * parent * node.common.transform;
    }
    Ok(())
}

fn paint(
    doc: &mut Document,
    ids: &[NodeId],
    mut change: impl FnMut(&mut VectorNode),
) -> Result<()> {
    for &id in ids {
        if let NodeKind::Vector(vector) = &mut doc.get_mut(id)?.kind {
            change(vector);
        }
    }
    Ok(())
}

/// A maths angle (radians, clockwise on screen since y points down) as the
/// panel shows it: degrees, counter-clockwise, in (-180, 180], with float
/// noise and negative zero cleaned up.
fn counter_clockwise_degrees(angle: f64) -> f64 {
    let mut degrees = -angle.to_degrees();
    if degrees <= -180.0 {
        degrees += 360.0;
    } else if degrees > 180.0 {
        degrees -= 360.0;
    }
    let rounded = (degrees * 1e6).round() / 1e6;
    if rounded == 0.0 { 0.0 } else { rounded }
}
