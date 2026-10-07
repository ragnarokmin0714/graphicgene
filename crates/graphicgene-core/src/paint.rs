//! What fills a shape or colours text: one colour, or a gradient.
//!
//! A gradient lives in the unit square of the shape's own bounding box —
//! (0, 0) its top-left corner, (1, 1) its bottom-right — so it stretches and
//! turns with the shape, exactly as SVG's default `objectBoundingBox`
//! gradients do; export writes it as one. A circle in that square is an
//! ellipse on a shape that is not square, in both.

use serde::{Deserialize, Serialize};

use crate::color::{LinearRgba, mix_srgb};
use crate::geom::{Point, Vec2};

/// A fill. In the file a solid colour is written bare, as every fill was
/// before gradients, so older documents read unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Paint {
    Solid(LinearRgba),
    Gradient(Gradient),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GradientKind {
    Linear,
    Radial,
}

/// Colours blended across the shape's box (see the module docs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub kind: GradientKind,
    /// Linear: where the first stop's colour lies. Radial: the centre.
    pub start: Point,
    /// Linear: where the last stop's colour lies. Radial: a point on the
    /// circle where the last colour is reached.
    pub end: Point,
    /// Two or more, in order of offset.
    pub stops: Vec<ColorStop>,
}

/// One colour of a gradient, at `offset` from 0 (the start) to 1 (the end).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorStop {
    pub offset: f64,
    pub color: LinearRgba,
}

impl Paint {
    /// The colour a gradient turns into when made solid: its first.
    pub fn first_color(&self) -> LinearRgba {
        match self {
            Paint::Solid(color) => *color,
            Paint::Gradient(gradient) => gradient.stops[0].color,
        }
    }
}

impl Gradient {
    /// Left to right across the box, or out from its centre to its edges.
    pub fn new(kind: GradientKind, stops: Vec<ColorStop>) -> Self {
        let (start, end) = match kind {
            GradientKind::Linear => (Point::new(0.0, 0.5), Point::new(1.0, 0.5)),
            GradientKind::Radial => (Point::new(0.5, 0.5), Point::new(1.0, 0.5)),
        };
        Self {
            kind,
            start,
            end,
            stops,
        }
    }

    /// From a colour to the same colour fully transparent — what a solid
    /// fill becomes when it is made a gradient, as in other editors.
    pub fn fading(kind: GradientKind, color: LinearRgba) -> Self {
        let clear = LinearRgba { a: 0.0, ..color };
        Self::new(
            kind,
            vec![
                ColorStop { offset: 0.0, color },
                ColorStop {
                    offset: 1.0,
                    color: clear,
                },
            ],
        )
    }

    /// The direction from start to end, in degrees counter-clockwise from
    /// pointing right — in the box's own terms, so on a box that is not
    /// square it is not the angle on screen.
    pub fn angle(&self) -> f64 {
        let d = self.end - self.start;
        let degrees = (-d.y).atan2(d.x).to_degrees();
        if degrees.abs() < 1e-9 { 0.0 } else { degrees }
    }

    /// Turn to `degrees` (see `angle`) about the midpoint of start and end,
    /// keeping their distance.
    pub fn set_angle(&mut self, degrees: f64) {
        let mid = self.start.midpoint(self.end);
        let half = (self.end - self.start).hypot() / 2.0;
        let radians = degrees.to_radians();
        let d = Vec2::new(radians.cos(), -radians.sin()) * half;
        self.start = mid - d;
        self.end = mid + d;
    }

    /// The colour at `offset`, between the stops around it, as the gradient
    /// is drawn there.
    pub fn color_at(&self, offset: f64) -> LinearRgba {
        let stops = &self.stops;
        let after = stops.iter().position(|s| s.offset >= offset);
        match after {
            None => stops[stops.len() - 1].color,
            Some(0) => stops[0].color,
            Some(i) => {
                let (a, b) = (stops[i - 1], stops[i]);
                let span = b.offset - a.offset;
                let t = if span > 0.0 {
                    ((offset - a.offset) / span) as f32
                } else {
                    0.0
                };
                mix_srgb(a.color, b.color, t)
            }
        }
    }
}

impl From<LinearRgba> for Paint {
    fn from(color: LinearRgba) -> Self {
        Paint::Solid(color)
    }
}
