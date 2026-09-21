//! Geometry types.
//!
//! The document works in `f64`; `f32` is only used at render time. A design
//! tool loses precision visibly when zoomed if the model itself is f32.
//!
//! Bezier maths comes from `kurbo` — hand-rolled curve and boolean code is a
//! multi-month detour with worse numerics.

pub use kurbo::{Affine, BezPath, Circle, Ellipse, Line, PathEl, Point, Rect, Shape, Size, Vec2};

/// Axis-aligned bounds used for invalidation and hit-testing.
pub type Bounds = Rect;

/// An empty rect that is safe to union into.
pub fn empty_bounds() -> Bounds {
    Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN)
}

pub fn is_empty_bounds(r: Bounds) -> bool {
    r.x0 > r.x1 || r.y0 > r.y1
}

pub fn union(a: Bounds, b: Bounds) -> Bounds {
    if is_empty_bounds(a) {
        return b;
    }
    if is_empty_bounds(b) {
        return a;
    }
    a.union(b)
}
