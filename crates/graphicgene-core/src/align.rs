//! Aligning and distributing the selection.
//!
//! Each node only moves, by a document-space translation composed through
//! its parent as a drag would, and all of them go in one batch: one undo
//! step. Bounds are the axis-aligned box of each node on the page, so a
//! rotated shape aligns by the box that encloses it, as in other editors.
//!
//! A group and something inside it, both selected, move as the group.

use crate::command::Command;
use crate::doc::Document;
use crate::error::Result;
use crate::geom::{Affine, Bounds, Rect, Vec2};
use crate::gesture::world_delta_command;
use crate::layers;
use crate::node::NodeId;

/// Below this, in document units, a node already counts as in place.
const SETTLED: f64 = 1e-9;

/// Which edge or centre line to bring into line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    /// Centres on one vertical line.
    CenterX,
    Right,
    Top,
    /// Centres on one horizontal line.
    CenterY,
    Bottom,
}

/// Which way to space the selection out evenly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribute {
    Horizontal,
    Vertical,
}

/// Move each selected node so the chosen edge or centre meets the target's:
/// the combined bounds of several nodes, or the artboard for one. None when
/// nothing would move.
pub fn align_command(doc: &Document, ids: &[NodeId], how: Align) -> Result<Option<Command>> {
    let boxes = boxes(doc, ids)?;
    let target = match boxes.as_slice() {
        [] => return Ok(None),
        [_] => {
            let size = doc.artboard();
            Rect::new(0.0, 0.0, size.width, size.height)
        }
        _ => boxes
            .iter()
            .map(|&(_, b)| b)
            .reduce(|a, b| a.union(b))
            .expect("at least two boxes"),
    };
    let moves = boxes.iter().map(|&(id, b)| {
        let offset = match how {
            Align::Left => Vec2::new(target.x0 - b.x0, 0.0),
            Align::CenterX => Vec2::new(target.center().x - b.center().x, 0.0),
            Align::Right => Vec2::new(target.x1 - b.x1, 0.0),
            Align::Top => Vec2::new(0.0, target.y0 - b.y0),
            Align::CenterY => Vec2::new(0.0, target.center().y - b.center().y),
            Align::Bottom => Vec2::new(0.0, target.y1 - b.y1),
        };
        (id, offset)
    });
    batch(doc, moves)
}

/// Space three or more nodes so the gaps between neighbours are equal,
/// keeping the first and last in place. Neighbours are taken in order of
/// their left (or top) edges. None for fewer than three, or when the gaps
/// are already equal.
pub fn distribute_command(
    doc: &Document,
    ids: &[NodeId],
    axis: Distribute,
) -> Result<Option<Command>> {
    let mut boxes = boxes(doc, ids)?;
    if boxes.len() < 3 {
        return Ok(None);
    }
    // (start, length) along the axis.
    let span = |b: &Bounds| match axis {
        Distribute::Horizontal => (b.x0, b.width()),
        Distribute::Vertical => (b.y0, b.height()),
    };
    boxes.sort_by(|a, b| span(&a.1).0.total_cmp(&span(&b.1).0));
    let (first, _) = span(&boxes[0].1);
    let end = boxes
        .iter()
        .map(|(_, b)| {
            let (start, length) = span(b);
            start + length
        })
        .fold(f64::NEG_INFINITY, f64::max);
    let lengths: f64 = boxes.iter().map(|(_, b)| span(b).1).sum();
    let gap = (end - first - lengths) / (boxes.len() - 1) as f64;

    let mut next = first;
    let moves: Vec<(NodeId, Vec2)> = boxes
        .iter()
        .map(|(id, b)| {
            let (start, length) = span(b);
            let shift = next - start;
            next += length + gap;
            let offset = match axis {
                Distribute::Horizontal => Vec2::new(shift, 0.0),
                Distribute::Vertical => Vec2::new(0.0, shift),
            };
            (*id, offset)
        })
        .collect();
    batch(doc, moves)
}

/// The topmost selected nodes with their bounds on the page.
fn boxes(doc: &Document, ids: &[NodeId]) -> Result<Vec<(NodeId, Bounds)>> {
    layers::roots(doc, ids)?
        .into_iter()
        .map(|id| Ok((id, doc.world_bounds(id)?)))
        .collect()
}

fn batch(
    doc: &Document,
    moves: impl IntoIterator<Item = (NodeId, Vec2)>,
) -> Result<Option<Command>> {
    let mut commands = Vec::new();
    for (id, offset) in moves {
        if offset.hypot() <= SETTLED {
            continue;
        }
        match world_delta_command(doc, &[id], Affine::translate(offset))? {
            Command::Batch(inner) => commands.extend(inner),
            command => commands.push(command),
        }
    }
    Ok((!commands.is_empty()).then_some(Command::Batch(commands)))
}
