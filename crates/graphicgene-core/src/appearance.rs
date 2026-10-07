//! A layer's paint as a whole — its fill, and its stroke with the stroke's
//! width and style — taken from one layer and given to others (the
//! eyedropper), set back to the defaults, or swapped.
//!
//! Each builds one `Batch` of what actually changes, so it is one undo
//! step, and nothing at all when nothing would change. Like the properties
//! panel, they paint the selected nodes themselves — shapes and text, not
//! what is inside a selected group. Text has a fill and no stroke.

use crate::color::LinearRgba;
use crate::command::Command;
use crate::doc::Document;
use crate::error::Result;
use crate::node::{NodeId, NodeKind, Stroke};
use crate::paint::Paint;
use crate::properties::DEFAULT_STROKE_WIDTH;

/// A fill and a stroke, as one layer has them.
#[derive(Debug, Clone, PartialEq)]
pub struct Appearance {
    pub fill: Option<Paint>,
    pub stroke: Option<Stroke>,
}

impl Appearance {
    /// What `id` is painted with; `None` for a group, which has no paint.
    pub fn of(doc: &Document, id: NodeId) -> Result<Option<Self>> {
        Ok(match &doc.get(id)?.kind {
            NodeKind::Vector(v) => Some(Self {
                fill: v.fill.clone(),
                stroke: v.stroke,
            }),
            NodeKind::Text(t) => Some(Self {
                fill: t.fill.clone(),
                stroke: None,
            }),
            NodeKind::Group(_) => None,
        })
    }
}

/// Give every node in `ids` `appearance` — the eyedropper.
pub fn apply_command(
    doc: &Document,
    ids: &[NodeId],
    appearance: &Appearance,
) -> Result<Option<Command>> {
    repaint(doc, ids, |_, _| Some(appearance.clone()))
}

/// Illustrator's defaults: a white fill and a black stroke one unit wide
/// for shapes, black for text.
pub fn default_command(doc: &Document, ids: &[NodeId]) -> Result<Option<Command>> {
    repaint(doc, ids, |kind, _| {
        Some(match kind {
            Kind::Shape => Appearance {
                fill: Some(Paint::Solid(LinearRgba::WHITE)),
                stroke: Some(Stroke::solid(LinearRgba::BLACK, DEFAULT_STROKE_WIDTH)),
            },
            Kind::Text => Appearance {
                fill: Some(Paint::Solid(LinearRgba::BLACK)),
                stroke: None,
            },
        })
    })
}

/// Swap each shape's fill colour and stroke colour. A stroke keeps its
/// width and style; a fill that becomes a stroke where there was none gets
/// the default width. A gradient becomes a stroke of its first colour —
/// strokes are one colour — so swapping back does not restore it. Text,
/// with no stroke to swap with, is left alone.
pub fn swap_command(doc: &Document, ids: &[NodeId]) -> Result<Option<Command>> {
    repaint(doc, ids, |kind, Appearance { fill, stroke }| {
        if kind == Kind::Text {
            return None;
        }
        let stroke_color = fill.as_ref().map(Paint::first_color);
        Some(Appearance {
            fill: stroke.map(|s| Paint::Solid(s.color)),
            stroke: stroke_color.map(|color| match stroke {
                Some(s) => Stroke { color, ..s },
                None => Stroke::solid(color, DEFAULT_STROKE_WIDTH),
            }),
        })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Shape,
    Text,
}

/// The commands that give each node in `ids` what `new` makes of its kind
/// and current appearance, where that differs; `None` from `new` leaves a
/// node alone. Text only ever takes the fill.
fn repaint(
    doc: &Document,
    ids: &[NodeId],
    new: impl Fn(Kind, Appearance) -> Option<Appearance>,
) -> Result<Option<Command>> {
    let mut commands = Vec::new();
    for &id in ids {
        let Some(old) = Appearance::of(doc, id)? else {
            continue;
        };
        let kind = match doc.get(id)?.kind {
            NodeKind::Text(_) => Kind::Text,
            _ => Kind::Shape,
        };
        let Some(new) = new(kind, old.clone()) else {
            continue;
        };
        if new.fill != old.fill {
            commands.push(Command::SetFill { id, fill: new.fill });
        }
        if kind == Kind::Shape && new.stroke != old.stroke {
            commands.push(Command::SetStroke {
                id,
                stroke: new.stroke,
            });
        }
    }
    Ok((!commands.is_empty()).then_some(Command::Batch(commands)))
}
