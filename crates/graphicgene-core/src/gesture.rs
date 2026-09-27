//! Drag gestures: move, scale and rotate the selection, draw a new shape, or
//! sweep a marquee.
//!
//! A gesture previews by writing straight into the document, bypassing the
//! journal, and commits as a single journal entry on release — so a drag made
//! of two hundred pointer events is one undo step. Cancelling puts back what
//! was there before the press.
//!
//! Nothing here knows about pointers, screens or zoom. The app layer feeds in
//! document-space points and which modifier keys are held.

use crate::color::LinearRgba;
use crate::command::{Command, Journal};
use crate::doc::Document;
use crate::error::Result;
use crate::geom::{
    Affine, BezPath, Ellipse, Point, Rect, Shape, Vec2, empty_bounds, is_empty_bounds, union,
};
use crate::hit;
use crate::node::{Node, NodeId};
use crate::selection::Selection;

/// A drag shorter than this, in document units, counts as a click. Clicking
/// with a shape tool places a default-sized shape instead of a sliver.
const CLICK_SLOP: f64 = 2.0;
const DEFAULT_SHAPE_SIZE: f64 = 100.0;
/// Shift-rotate snaps the frame's absolute angle to multiples of this.
const ROTATE_SNAP: f64 = std::f64::consts::PI / 12.0;
/// Scale factors are kept away from zero so every transform stays invertible;
/// hit-testing depends on inverting them.
const MIN_SCALE: f64 = 1e-3;

/// Modifier keys held during a drag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Constrain: axis-lock a move, keep proportions, snap rotation, draw a
    /// square or circle.
    pub shift: bool,
    /// From the centre: scale or draw around the middle instead of the
    /// opposite edge.
    pub alt: bool,
}

/// The box a selection is manipulated through: `rect` in frame space, mapped
/// into the document by `transform`.
///
/// A single node's frame follows the node's own rotation and scale; several
/// nodes share an axis-aligned frame around their combined bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub transform: Affine,
    pub rect: Rect,
}

impl Frame {
    pub fn of(doc: &Document, ids: &[NodeId]) -> Result<Option<Frame>> {
        if let [id] = ids
            && let Some(rect) = doc.get(*id)?.local_bounds()
        {
            return Ok(Some(Frame {
                transform: doc.world_transform(*id)?,
                rect,
            }));
        }
        let mut bounds = empty_bounds();
        for &id in ids {
            bounds = union(bounds, doc.world_bounds(id)?);
        }
        if is_empty_bounds(bounds) {
            return Ok(None);
        }
        Ok(Some(Frame {
            transform: Affine::IDENTITY,
            rect: bounds,
        }))
    }

    /// Frame-space point at unit coordinates: (0, 0) is the top-left corner,
    /// (1, 1) the bottom-right, (0.5, 0.5) the centre.
    fn local_at(&self, u: f64, v: f64) -> Point {
        Point::new(
            self.rect.x0 + u * self.rect.width(),
            self.rect.y0 + v * self.rect.height(),
        )
    }

    /// Document-space point at unit coordinates; see `local_at`.
    pub fn point_at(&self, u: f64, v: f64) -> Point {
        self.transform * self.local_at(u, v)
    }

    /// Corners in document space: top-left, top-right, bottom-right,
    /// bottom-left, in frame terms (a rotated frame's "top" may not be up).
    pub fn corners(&self) -> [Point; 4] {
        [
            self.point_at(0.0, 0.0),
            self.point_at(1.0, 0.0),
            self.point_at(1.0, 1.0),
            self.point_at(0.0, 1.0),
        ]
    }

    /// Edge lengths in document units: what a size readout shows.
    pub fn size(&self) -> (f64, f64) {
        let [top_left, top_right, _, bottom_left] = self.corners();
        (
            (top_right - top_left).hypot(),
            (bottom_left - top_left).hypot(),
        )
    }

    /// The frame's rotation in radians: the angle of its top edge, clockwise
    /// on screen since the document's y axis points down.
    pub fn angle(&self) -> f64 {
        let [a, b, ..] = self.transform.as_coeffs();
        b.atan2(a)
    }

    fn then(self, delta: Affine) -> Frame {
        Frame {
            transform: delta * self.transform,
            rect: self.rect,
        }
    }
}

/// What a drag on the selection does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransformKind {
    Move,
    /// Drag the handle at unit coordinates `(u, v)` of the frame — a corner is
    /// `(0|1, 0|1)`, an edge midpoint has one coordinate at `0.5`.
    Scale {
        u: f64,
        v: f64,
    },
    Rotate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Rect,
    Ellipse,
}

impl ShapeKind {
    fn name(self) -> &'static str {
        match self {
            ShapeKind::Rect => "Rectangle",
            ShapeKind::Ellipse => "Ellipse",
        }
    }

    fn path(self, rect: Rect) -> BezPath {
        match self {
            ShapeKind::Rect => rect.to_path(0.1),
            ShapeKind::Ellipse => Ellipse::from_rect(rect).to_path(0.1),
        }
    }
}

#[derive(Debug, Clone)]
enum Kind {
    Transform {
        kind: TransformKind,
        /// The frame at the press, which every update is measured against.
        frame: Frame,
        originals: Vec<Original>,
        /// The document-space change the current pointer position implies.
        delta: Affine,
    },
    Create {
        shape: ShapeKind,
        id: NodeId,
    },
    Marquee {
        additive: bool,
    },
}

/// A node's transform at the press, and its parent's world transform, which
/// together turn a document-space delta back into a local transform.
#[derive(Debug, Clone, Copy)]
struct Original {
    id: NodeId,
    local: Affine,
    parent_world: Affine,
}

/// One press-drag-release interaction.
#[derive(Debug, Clone)]
pub struct Gesture {
    kind: Kind,
    start: Point,
    current: Point,
    /// The selection at the press, restored on cancel and extended by an
    /// additive marquee.
    base: Vec<NodeId>,
}

impl Gesture {
    /// Start moving, scaling or rotating the selection. `None` if nothing
    /// selected has any extent to manipulate.
    pub fn transform(
        doc: &Document,
        selection: &Selection,
        kind: TransformKind,
        start: Point,
    ) -> Result<Option<Gesture>> {
        let ids = selection.ids();
        let Some(frame) = Frame::of(doc, ids)? else {
            return Ok(None);
        };
        let mut originals = Vec::with_capacity(ids.len());
        for &id in ids {
            originals.push(Original {
                id,
                local: doc.get(id)?.common.transform,
                parent_world: doc.parent_world_transform(id)?,
            });
        }
        Ok(Some(Gesture {
            kind: Kind::Transform {
                kind,
                frame,
                originals,
                delta: Affine::IDENTITY,
            },
            start,
            current: start,
            base: ids.to_vec(),
        }))
    }

    /// Start drawing a new shape from `start`. The node is attached right away
    /// so it renders while being drawn, but it only reaches the journal on
    /// commit, and it becomes the selection.
    pub fn create(
        doc: &mut Document,
        selection: &mut Selection,
        shape: ShapeKind,
        fill: LinearRgba,
        start: Point,
    ) -> Result<Gesture> {
        let root = doc.root();
        let index = doc.children_of(root)?.len();
        let path = shape.path(Rect::from_points(start, start));
        let id = doc.insert_detached(Node::vector(shape.name(), path, Some(fill)));
        doc.attach(id, root, index)?;
        let base = selection.ids().to_vec();
        selection.set([id]);
        Ok(Gesture {
            kind: Kind::Create { shape, id },
            start,
            current: start,
            base,
        })
    }

    /// Start a marquee. With `additive`, what it sweeps is added to the
    /// current selection rather than replacing it.
    pub fn marquee(selection: &Selection, start: Point, additive: bool) -> Gesture {
        Gesture {
            kind: Kind::Marquee { additive },
            start,
            current: start,
            base: selection.ids().to_vec(),
        }
    }

    /// Follow the pointer to `point`, previewing the result in the document.
    pub fn update(
        &mut self,
        doc: &mut Document,
        selection: &mut Selection,
        point: Point,
        modifiers: Modifiers,
    ) -> Result<()> {
        self.current = point;
        match &mut self.kind {
            Kind::Transform {
                kind,
                frame,
                originals,
                delta,
            } => {
                *delta = transform_delta(*kind, frame, self.start, point, modifiers);
                for original in originals.iter() {
                    doc.get_mut(original.id)?.common.transform = moved(original, *delta);
                }
            }
            Kind::Create { shape, id } => {
                let rect = drawn_rect(self.start, point, modifiers);
                doc.write_path(*id, shape.path(rect))?;
            }
            Kind::Marquee { additive } => {
                let hits = hit::nodes_in_rect(doc, Rect::from_points(self.start, point))?;
                if *additive {
                    selection.set(self.base.iter().copied().chain(hits));
                } else {
                    selection.set(hits);
                }
            }
        }
        Ok(())
    }

    /// Finish the gesture as a single journal entry. Returns the new node's
    /// id when a shape was drawn.
    pub fn commit(
        self,
        doc: &mut Document,
        journal: &mut Journal,
        selection: &mut Selection,
    ) -> Result<Option<NodeId>> {
        let dragged = (self.current - self.start).hypot() >= CLICK_SLOP;
        match self.kind {
            Kind::Transform {
                originals, delta, ..
            } => {
                // Put the press-time transforms back, then apply the final
                // ones through the journal so undo records where they came from.
                for original in &originals {
                    doc.get_mut(original.id)?.common.transform = original.local;
                }
                if delta != Affine::IDENTITY {
                    let commands = originals
                        .iter()
                        .map(|original| Command::SetTransform {
                            id: original.id,
                            transform: moved(original, delta),
                        })
                        .collect();
                    journal.execute(doc, Command::Batch(commands))?;
                }
                Ok(None)
            }
            Kind::Create { shape, id } => {
                if !dragged {
                    let size = (DEFAULT_SHAPE_SIZE, DEFAULT_SHAPE_SIZE);
                    doc.write_path(id, shape.path(Rect::from_origin_size(self.start, size)))?;
                }
                // Re-attach through the journal: undo then detaches the node
                // and redo re-attaches the same id, path and all.
                let (parent, index) = doc.detach(id)?;
                journal.execute(doc, Command::Attach { id, parent, index })?;
                selection.set([id]);
                Ok(Some(id))
            }
            Kind::Marquee { .. } => Ok(None),
        }
    }

    /// Abandon the gesture, leaving the document and selection as they were
    /// at the press.
    pub fn cancel(self, doc: &mut Document, selection: &mut Selection) -> Result<()> {
        match self.kind {
            Kind::Transform { originals, .. } => {
                for original in &originals {
                    doc.get_mut(original.id)?.common.transform = original.local;
                }
            }
            Kind::Create { id, .. } => {
                // Left detached in the arena; save purges unreachable nodes.
                doc.detach(id)?;
            }
            Kind::Marquee { .. } => {}
        }
        selection.set(self.base);
        Ok(())
    }

    /// Where the selection frame is right now, for drawing handles during a
    /// transform. A rotated multi-selection keeps its rotated frame for the
    /// whole drag instead of re-fitting an ever-growing axis-aligned box.
    pub fn frame(&self) -> Option<Frame> {
        match &self.kind {
            Kind::Transform { frame, delta, .. } => Some(frame.then(*delta)),
            _ => None,
        }
    }

    /// The marquee rectangle, while one is being swept.
    pub fn marquee_rect(&self) -> Option<Rect> {
        match self.kind {
            Kind::Marquee { .. } => Some(Rect::from_points(self.start, self.current)),
            _ => None,
        }
    }

    /// A short name for the UI, e.g. to pick a cursor.
    pub fn label(&self) -> &'static str {
        match &self.kind {
            Kind::Transform { kind, .. } => match kind {
                TransformKind::Move => "move",
                TransformKind::Scale { .. } => "scale",
                TransformKind::Rotate => "rotate",
            },
            Kind::Create { .. } => "create",
            Kind::Marquee { .. } => "marquee",
        }
    }
}

/// A command that applies the document-space `delta` to each node in `ids` —
/// what arrow-key nudges use. One command, so one undo step.
pub fn world_delta_command(doc: &Document, ids: &[NodeId], delta: Affine) -> Result<Command> {
    let mut commands = Vec::with_capacity(ids.len());
    for &id in ids {
        let original = Original {
            id,
            local: doc.get(id)?.common.transform,
            parent_world: doc.parent_world_transform(id)?,
        };
        commands.push(Command::SetTransform {
            id,
            transform: moved(&original, delta),
        });
    }
    Ok(Command::Batch(commands))
}

/// The local transform that puts `original` where `delta` moves it in
/// document space.
fn moved(original: &Original, delta: Affine) -> Affine {
    original.parent_world.inverse() * delta * original.parent_world * original.local
}

fn transform_delta(
    kind: TransformKind,
    frame: &Frame,
    start: Point,
    point: Point,
    modifiers: Modifiers,
) -> Affine {
    match kind {
        TransformKind::Move => {
            let mut d = point - start;
            if modifiers.shift {
                if d.x.abs() >= d.y.abs() {
                    d.y = 0.0;
                } else {
                    d.x = 0.0;
                }
            }
            Affine::translate(d)
        }

        TransformKind::Scale { u, v } => {
            // Work in frame space, where the frame is an axis-aligned rect and
            // scaling is a plain scale about an anchor point.
            let q = frame.transform.inverse() * point;
            let handle = frame.local_at(u, v);
            let anchor = if modifiers.alt {
                frame.local_at(0.5, 0.5)
            } else {
                frame.local_at(1.0 - u, 1.0 - v)
            };
            let on_x = !is_mid(u);
            let on_y = !is_mid(v);
            let factor = |q: f64, handle: f64, anchor: f64| {
                let span = handle - anchor;
                if span.abs() < 1e-9 {
                    1.0
                } else {
                    (q - anchor) / span
                }
            };
            let mut sx = if on_x {
                factor(q.x, handle.x, anchor.x)
            } else {
                1.0
            };
            let mut sy = if on_y {
                factor(q.y, handle.y, anchor.y)
            } else {
                1.0
            };
            if modifiers.shift {
                match (on_x, on_y) {
                    (true, true) => {
                        let m = sx.abs().max(sy.abs());
                        sx = m.copysign(sx);
                        sy = m.copysign(sy);
                    }
                    (true, false) => sy = sx.abs(),
                    (false, true) => sx = sy.abs(),
                    (false, false) => {}
                }
            }
            let scale = Affine::translate(anchor.to_vec2())
                * Affine::scale_non_uniform(away_from_zero(sx), away_from_zero(sy))
                * Affine::translate(-anchor.to_vec2());
            frame.transform * scale * frame.transform.inverse()
        }

        TransformKind::Rotate => {
            let center = frame.point_at(0.5, 0.5);
            let from = (start - center).atan2();
            let to = (point - center).atan2();
            let mut angle = to - from;
            if modifiers.shift {
                let base = frame.angle();
                angle = ((base + angle) / ROTATE_SNAP).round() * ROTATE_SNAP - base;
            }
            Affine::rotate_about(angle, center)
        }
    }
}

/// The rectangle a shape tool draws between the press and the pointer.
fn drawn_rect(start: Point, point: Point, modifiers: Modifiers) -> Rect {
    let mut d = point - start;
    if modifiers.shift {
        let side = d.x.abs().max(d.y.abs());
        d = Vec2::new(side.copysign(d.x), side.copysign(d.y));
    }
    if modifiers.alt {
        Rect::from_center_size(start, (2.0 * d.x.abs(), 2.0 * d.y.abs()))
    } else {
        Rect::from_points(start, start + d)
    }
}

fn is_mid(t: f64) -> bool {
    (t - 0.5).abs() < 1e-9
}

fn away_from_zero(s: f64) -> f64 {
    if s.abs() < MIN_SCALE {
        MIN_SCALE.copysign(s)
    } else {
        s
    }
}
