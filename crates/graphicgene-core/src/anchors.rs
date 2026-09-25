//! The anchor view of a path: what the pen tool and path editing work on.
//!
//! The document stores plain `BezPath`s — that is the file format and what the
//! renderer draws. Editing needs something closer to what the user sees:
//! anchor points, each with an optional incoming and outgoing handle. This
//! module converts between the two. A round trip keeps the geometry;
//! quadratic segments come back as the equivalent cubics.
//!
//! One thing a `BezPath` cannot hold: the outer handles of an open path's end
//! anchors (the first anchor's incoming, the last one's outgoing), since no
//! segment uses them. They are lost on conversion. That only matters once a
//! path can be continued from its end, which v0.1 does not do.

use kurbo::{CubicBez, Line, ParamCurve, ParamCurveNearest, PathSeg};

use crate::geom::{BezPath, PathEl, Point, Vec2};

/// Handles closer than this to their anchor are treated as absent.
const SAME_POINT: f64 = 1e-9;
/// An open subpath ending this close to its start is treated as closed.
/// kurbo's own ellipse ends a float's width from where it began and never
/// emits ClosePath, so exact equality would miss it.
const COINCIDENT: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub point: Point,
    /// Controls the curve arriving at this anchor.
    pub handle_in: Option<Point>,
    /// Controls the curve leaving this anchor.
    pub handle_out: Option<Point>,
}

impl Anchor {
    pub fn corner(point: Point) -> Self {
        Self {
            point,
            handle_in: None,
            handle_out: None,
        }
    }

    pub fn handle(&self, side: HandleSide) -> Option<Point> {
        match side {
            HandleSide::In => self.handle_in,
            HandleSide::Out => self.handle_out,
        }
    }

    pub fn set_handle(&mut self, side: HandleSide, handle: Option<Point>) {
        let handle = handle.filter(|h| (*h - self.point).hypot() > SAME_POINT);
        match side {
            HandleSide::In => self.handle_in = handle,
            HandleSide::Out => self.handle_out = handle,
        }
    }

    /// Both handles present and pointing in opposite directions: dragging one
    /// should swing the other with it.
    pub fn is_smooth(&self) -> bool {
        let (Some(handle_in), Some(handle_out)) = (self.handle_in, self.handle_out) else {
            return false;
        };
        let a = handle_in - self.point;
        let b = handle_out - self.point;
        a.cross(b).abs() <= 1e-6 * a.hypot() * b.hypot() && a.dot(b) < 0.0
    }

    /// Move the anchor, carrying its handles along.
    pub fn translate(&mut self, delta: Vec2) {
        self.point += delta;
        self.handle_in = self.handle_in.map(|h| h + delta);
        self.handle_out = self.handle_out.map(|h| h + delta);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleSide {
    In,
    Out,
}

impl HandleSide {
    pub fn opposite(self) -> Self {
        match self {
            HandleSide::In => HandleSide::Out,
            HandleSide::Out => HandleSide::In,
        }
    }
}

/// Addresses one anchor: which subpath, and its position in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnchorId {
    pub subpath: usize,
    pub index: usize,
}

/// What a point on the path's editing overlay lands on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathHit {
    Handle(AnchorId, HandleSide),
    Anchor(AnchorId),
    /// The segment leaving anchor `from`, at curve parameter `t`.
    Segment {
        from: AnchorId,
        t: f64,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Subpath {
    pub anchors: Vec<Anchor>,
    pub closed: bool,
}

impl Subpath {
    fn starting_at(point: Point) -> Self {
        Self {
            anchors: vec![Anchor::corner(point)],
            closed: false,
        }
    }

    fn segment_count(&self) -> usize {
        match self.anchors.len() {
            0 | 1 => 0,
            n if self.closed => n,
            n => n - 1,
        }
    }

    /// Segment `i` runs from anchor `i` to the next one, wrapping on a closed
    /// subpath.
    fn segment(&self, i: usize) -> PathSeg {
        let a = &self.anchors[i];
        let b = &self.anchors[(i + 1) % self.anchors.len()];
        segment_between(a, b)
    }
}

/// A path as anchors with handles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnchorPath {
    pub subpaths: Vec<Subpath>,
}

impl AnchorPath {
    pub fn from_bez(path: &BezPath) -> Self {
        let mut subpaths: Vec<Subpath> = Vec::new();
        for el in path.elements() {
            // A drawing command with no MoveTo before it starts a subpath at
            // its own end point (kurbo tolerates such paths; so do we).
            if !matches!(el, PathEl::MoveTo(_) | PathEl::ClosePath) && subpaths.is_empty() {
                if let Some(p) = el.end_point() {
                    subpaths.push(Subpath::starting_at(p));
                }
                continue;
            }
            match *el {
                PathEl::MoveTo(p) => subpaths.push(Subpath::starting_at(p)),
                PathEl::LineTo(p) => last(&mut subpaths).anchors.push(Anchor::corner(p)),
                PathEl::QuadTo(c, p) => {
                    // The cubic with the same shape: control points two thirds
                    // of the way from each end towards the quad's control.
                    let sub = last(&mut subpaths);
                    let prev = sub
                        .anchors
                        .last_mut()
                        .expect("subpaths start with an anchor");
                    let out = prev.point + (c - prev.point) * (2.0 / 3.0);
                    prev.set_handle(HandleSide::Out, Some(out));
                    let mut anchor = Anchor::corner(p);
                    anchor.set_handle(HandleSide::In, Some(p + (c - p) * (2.0 / 3.0)));
                    sub.anchors.push(anchor);
                }
                PathEl::CurveTo(c1, c2, p) => {
                    let sub = last(&mut subpaths);
                    let prev = sub
                        .anchors
                        .last_mut()
                        .expect("subpaths start with an anchor");
                    prev.set_handle(HandleSide::Out, Some(c1));
                    let mut anchor = Anchor::corner(p);
                    anchor.set_handle(HandleSide::In, Some(c2));
                    sub.anchors.push(anchor);
                }
                PathEl::ClosePath => {
                    if let Some(sub) = subpaths.last_mut() {
                        sub.closed = true;
                        merge_closing_anchor(sub);
                    }
                }
            }
        }
        for sub in &mut subpaths {
            if !sub.closed && sub.anchors.len() > 2 && merge_closing_anchor(sub) {
                sub.closed = true;
            }
        }
        Self { subpaths }
    }

    pub fn to_bez(&self) -> BezPath {
        let mut path = BezPath::new();
        for sub in &self.subpaths {
            let Some(first) = sub.anchors.first() else {
                continue;
            };
            path.move_to(first.point);
            for i in 0..sub.segment_count() {
                match sub.segment(i) {
                    PathSeg::Line(line) => path.line_to(line.p1),
                    PathSeg::Cubic(c) => path.curve_to(c.p1, c.p2, c.p3),
                    PathSeg::Quad(q) => path.quad_to(q.p1, q.p2),
                }
            }
            if sub.closed {
                path.close_path();
            }
        }
        path
    }

    /// Total number of anchors.
    pub fn len(&self) -> usize {
        self.subpaths.iter().map(|s| s.anchors.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn ids(&self) -> impl Iterator<Item = AnchorId> + '_ {
        self.subpaths.iter().enumerate().flat_map(|(subpath, sub)| {
            (0..sub.anchors.len()).map(move |index| AnchorId { subpath, index })
        })
    }

    pub fn get(&self, id: AnchorId) -> Option<&Anchor> {
        self.subpaths.get(id.subpath)?.anchors.get(id.index)
    }

    pub fn get_mut(&mut self, id: AnchorId) -> Option<&mut Anchor> {
        self.subpaths.get_mut(id.subpath)?.anchors.get_mut(id.index)
    }

    /// What lies at `point`, within `tolerance`.
    ///
    /// Handles are only live on the anchors in `with_handles` (the ones the
    /// overlay shows them for), and win over anchors, which win over
    /// segments: the smaller target has to be reachable.
    pub fn hit(&self, point: Point, tolerance: f64, with_handles: &[AnchorId]) -> Option<PathHit> {
        let near = |p: Point| (p - point).hypot() <= tolerance;
        for &id in with_handles {
            let Some(anchor) = self.get(id) else { continue };
            for side in [HandleSide::In, HandleSide::Out] {
                if anchor.handle(side).is_some_and(near) {
                    return Some(PathHit::Handle(id, side));
                }
            }
        }

        let nearest_anchor = self
            .ids()
            .map(|id| {
                (
                    id,
                    (self
                        .get(id)
                        .map_or(f64::INFINITY, |a| (a.point - point).hypot())),
                )
            })
            .filter(|(_, d)| *d <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = nearest_anchor {
            return Some(PathHit::Anchor(id));
        }

        let mut best: Option<(PathHit, f64)> = None;
        for (subpath, sub) in self.subpaths.iter().enumerate() {
            for index in 0..sub.segment_count() {
                let nearest = sub.segment(index).nearest(point, 1e-6);
                if nearest.distance_sq <= tolerance * tolerance
                    && best.is_none_or(|(_, d)| nearest.distance_sq < d)
                {
                    let from = AnchorId { subpath, index };
                    best = Some((PathHit::Segment { from, t: nearest.t }, nearest.distance_sq));
                }
            }
        }
        best.map(|(hit, _)| hit)
    }

    /// Insert an anchor on the segment leaving `from`, at parameter `t`,
    /// without changing the shape. Returns the new anchor's id.
    pub fn split_segment(&mut self, from: AnchorId, t: f64) -> AnchorId {
        let sub = &mut self.subpaths[from.subpath];
        let i = from.index;
        let j = (i + 1) % sub.anchors.len();
        let anchor = match sub.segment(i) {
            PathSeg::Cubic(c) => {
                let left = c.subsegment(0.0..t);
                let right = c.subsegment(t..1.0);
                sub.anchors[i].set_handle(HandleSide::Out, Some(left.p1));
                sub.anchors[j].set_handle(HandleSide::In, Some(right.p2));
                let mut anchor = Anchor::corner(left.p3);
                anchor.set_handle(HandleSide::In, Some(left.p2));
                anchor.set_handle(HandleSide::Out, Some(right.p1));
                anchor
            }
            seg => Anchor::corner(seg.eval(t)),
        };
        // On the closing segment, i + 1 is one past the end: a push.
        sub.anchors.insert(i + 1, anchor);
        AnchorId {
            subpath: from.subpath,
            index: i + 1,
        }
    }

    /// Remove anchors. Their neighbours are joined directly, keeping their
    /// own handles; subpaths left with fewer than two anchors disappear.
    pub fn remove(&mut self, ids: &[AnchorId]) {
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        for id in ids.iter().rev() {
            if let Some(sub) = self.subpaths.get_mut(id.subpath)
                && id.index < sub.anchors.len()
            {
                sub.anchors.remove(id.index);
            }
        }
        self.subpaths.retain(|sub| sub.anchors.len() >= 2);
    }

    /// Turn a corner into a smooth point, or a smooth point into a corner.
    ///
    /// New handles lie along the line between the neighbours, a third of the
    /// way to each — the usual starting point in Illustrator and Figma.
    pub fn toggle_smooth(&mut self, id: AnchorId) {
        let (prev, next) = self.neighbours(id);
        let Some(anchor) = self.get_mut(id) else {
            return;
        };
        if anchor.handle_in.is_some() || anchor.handle_out.is_some() {
            anchor.handle_in = None;
            anchor.handle_out = None;
            return;
        }
        let direction = match (prev, next) {
            (Some(p), Some(n)) => n - p,
            (Some(p), None) => anchor.point - p,
            (None, Some(n)) => n - anchor.point,
            (None, None) => return,
        };
        if direction.hypot() <= SAME_POINT {
            return;
        }
        let direction = direction.normalize();
        if let Some(p) = prev {
            let reach = (anchor.point - p).hypot() / 3.0;
            anchor.set_handle(HandleSide::In, Some(anchor.point - direction * reach));
        }
        if let Some(n) = next {
            let reach = (n - anchor.point).hypot() / 3.0;
            anchor.set_handle(HandleSide::Out, Some(anchor.point + direction * reach));
        }
    }

    /// The points of the anchors before and after `id`, wrapping on a
    /// closed subpath.
    fn neighbours(&self, id: AnchorId) -> (Option<Point>, Option<Point>) {
        let Some(sub) = self.subpaths.get(id.subpath) else {
            return (None, None);
        };
        let n = sub.anchors.len();
        if n < 2 || id.index >= n {
            return (None, None);
        }
        let prev = if id.index > 0 {
            Some(id.index - 1)
        } else if sub.closed {
            Some(n - 1)
        } else {
            None
        };
        let next = if id.index + 1 < n {
            Some(id.index + 1)
        } else if sub.closed {
            Some(0)
        } else {
            None
        };
        (
            prev.map(|i| sub.anchors[i].point),
            next.map(|i| sub.anchors[i].point),
        )
    }
}

/// Paths that draw their closing segment explicitly end on the start point;
/// that is one anchor, not two. Returns whether a merge happened.
fn merge_closing_anchor(sub: &mut Subpath) -> bool {
    if sub.anchors.len() < 2 {
        return false;
    }
    let first = sub.anchors[0].point;
    let end = sub.anchors[sub.anchors.len() - 1];
    if (end.point - first).hypot() > COINCIDENT {
        return false;
    }
    sub.anchors.pop();
    sub.anchors[0].handle_in = end.handle_in;
    true
}

fn last(subpaths: &mut [Subpath]) -> &mut Subpath {
    subpaths
        .last_mut()
        .expect("from_bez starts a subpath first")
}

fn segment_between(a: &Anchor, b: &Anchor) -> PathSeg {
    if a.handle_out.is_none() && b.handle_in.is_none() {
        PathSeg::Line(Line::new(a.point, b.point))
    } else {
        PathSeg::Cubic(CubicBez::new(
            a.point,
            a.handle_out.unwrap_or(a.point),
            b.handle_in.unwrap_or(b.point),
            b.point,
        ))
    }
}

/// `to`, moved onto the nearest 45° line through `from`. What Shift does to
/// a new pen anchor or a dragged handle.
pub fn constrain_45(from: Point, to: Point) -> Point {
    let d = to - from;
    if d.hypot() <= SAME_POINT {
        return to;
    }
    let step = std::f64::consts::FRAC_PI_4;
    let direction = Vec2::from_angle((d.atan2() / step).round() * step);
    from + direction * d.dot(direction)
}
