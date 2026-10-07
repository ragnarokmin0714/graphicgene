//! Snapping: while a drag moves the selection or draws a shape, its edges
//! and centre lines catch on other layers' and the artboard's, and the line
//! each axis caught on is drawn as a guide.
//!
//! The lines to catch on are gathered once, when the drag starts; each move
//! of the pointer only scans them, allocating nothing.

use crate::doc::Document;
use crate::error::Result;
use crate::geom::{Point, Rect, Vec2};
use crate::node::{NodeId, NodeKind};

/// A line something can snap to: where it is on its axis, and how far it
/// runs along the other one — what a guide drawn on it spans.
#[derive(Debug, Clone, Copy)]
struct Line {
    at: f64,
    from: f64,
    to: f64,
}

/// A guide over the canvas while snapped, in document space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guide {
    pub from: Point,
    pub to: Point,
}

/// What snapped in one pointer move: the shift that makes it so, and a
/// guide for each axis that caught.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Snapped {
    pub offset: Vec2,
    pub guides: [Option<Guide>; 2],
}

/// The lines a drag can snap to.
#[derive(Debug, Clone, Default)]
pub struct Snapper {
    /// Vertical lines, by their x.
    xs: Vec<Line>,
    /// Horizontal lines, by their y.
    ys: Vec<Line>,
}

impl Snapper {
    /// The artboard's edges and centre lines, and those of every visible
    /// shape and text except `moving` and what is inside it — what is being
    /// dragged cannot snap to itself.
    pub fn new(doc: &Document, moving: &[NodeId]) -> Result<Self> {
        let size = doc.artboard();
        let mut snapper = Snapper::default();
        snapper.add(Rect::new(0.0, 0.0, size.width, size.height));
        for id in doc.walk() {
            let node = doc.get(id)?;
            if matches!(node.kind, NodeKind::Group(_)) || !is_shown(doc, id)? {
                continue;
            }
            if under_any(doc, id, moving)? {
                continue;
            }
            snapper.add(doc.world_bounds(id)?);
        }
        Ok(snapper)
    }

    fn add(&mut self, b: Rect) {
        for at in [b.x0, b.center().x, b.x1] {
            self.xs.push(Line {
                at,
                from: b.y0,
                to: b.y1,
            });
        }
        for at in [b.y0, b.center().y, b.y1] {
            self.ys.push(Line {
                at,
                from: b.x0,
                to: b.x1,
            });
        }
    }

    /// Snap a box being moved: its left, centre or right — whichever is
    /// nearest a line within `tolerance` — and likewise across.
    pub fn snap_box(&self, b: Rect, tolerance: f64) -> Snapped {
        let x = nearest(&self.xs, [b.x0, b.center().x, b.x1], tolerance);
        let y = nearest(&self.ys, [b.y0, b.center().y, b.y1], tolerance);
        let offset = Vec2::new(x.map_or(0.0, |(d, _)| d), y.map_or(0.0, |(d, _)| d));
        let moved = b + offset;
        Snapped {
            offset,
            guides: [
                x.map(|(_, line)| vertical(line, moved.y0, moved.y1)),
                y.map(|(_, line)| horizontal(line, moved.x0, moved.x1)),
            ],
        }
    }

    /// Snap a point: a corner of a shape being drawn.
    pub fn snap_point(&self, p: Point, tolerance: f64) -> Snapped {
        self.snap_box(Rect::from_points(p, p), tolerance)
    }
}

/// The smallest shift within `tolerance` that puts one of `edges` on a
/// line, with the line.
fn nearest(lines: &[Line], edges: [f64; 3], tolerance: f64) -> Option<(f64, Line)> {
    let mut best: Option<(f64, Line)> = None;
    for line in lines {
        for edge in edges {
            let d = line.at - edge;
            if d.abs() <= tolerance && best.is_none_or(|(b, _)| d.abs() < b.abs()) {
                best = Some((d, *line));
            }
        }
    }
    best
}

/// A guide along `line`, long enough to reach both it and the moved box.
fn vertical(line: Line, y0: f64, y1: f64) -> Guide {
    Guide {
        from: Point::new(line.at, line.from.min(y0)),
        to: Point::new(line.at, line.to.max(y1)),
    }
}

fn horizontal(line: Line, x0: f64, x1: f64) -> Guide {
    Guide {
        from: Point::new(line.from.min(x0), line.at),
        to: Point::new(line.to.max(x1), line.at),
    }
}

/// Visible, and so is everything it is inside.
fn is_shown(doc: &Document, id: NodeId) -> Result<bool> {
    let mut at = Some(id);
    while let Some(id) = at {
        let node = doc.get(id)?;
        if !node.common.visible {
            return Ok(false);
        }
        at = node.common.parent;
    }
    Ok(true)
}

/// `id` is one of `moving`, or inside one.
fn under_any(doc: &Document, id: NodeId, moving: &[NodeId]) -> Result<bool> {
    let mut at = Some(id);
    while let Some(id) = at {
        if moving.contains(&id) {
            return Ok(true);
        }
        at = doc.get(id)?.common.parent;
    }
    Ok(false)
}
