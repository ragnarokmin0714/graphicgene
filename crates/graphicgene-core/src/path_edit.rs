//! Editing an existing path's anchors and handles.
//!
//! Entered on one vector node. A press lands on a handle (drag it), an anchor
//! (select it and drag the selected anchors) or a segment (insert an anchor
//! there and drag it). Like the other gestures, a drag previews in the
//! document and commits as a single `SetPath` on release.
//!
//! Points arrive in document space and are mapped into the node's local
//! space, so editing works the same on a moved, scaled or rotated node.

use crate::anchors::{AnchorId, AnchorPath, HandleSide, PathHit, constrain_45};
use crate::command::{Command, Journal};
use crate::doc::Document;
use crate::error::Result;
use crate::geom::{Affine, BezPath, Point, Vec2};
use crate::gesture::Modifiers;
use crate::node::{NodeId, NodeKind};

/// What a press in path-edit mode landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressOutcome {
    Handle,
    Anchor,
    /// A segment: an anchor was inserted there.
    Segment,
    Miss,
}

/// What a delete did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    Nothing,
    Anchors,
    /// Every anchor went: the caller should delete the node itself.
    EmptiedPath,
}

#[derive(Debug, Clone)]
struct Drag {
    /// The path before the press, which the commit records as the undo state.
    original: BezPath,
    /// The anchors at the start of the drag, after any insertion.
    anchors: AnchorPath,
    start: Point,
    kind: DragKind,
}

#[derive(Debug, Clone, Copy)]
enum DragKind {
    Anchors,
    Handle {
        anchor: AnchorId,
        side: HandleSide,
        smooth: bool,
    },
}

/// Everything the overlay needs, in document space.
#[derive(Debug, Clone, Default)]
pub struct EditView {
    pub outline: BezPath,
    /// Each anchor and whether it is selected.
    pub anchors: Vec<(Point, bool)>,
    /// (anchor, handle) pairs for the selected anchors.
    pub handles: Vec<(Point, Point)>,
}

#[derive(Debug, Clone)]
pub struct PathEdit {
    id: NodeId,
    selected: Vec<AnchorId>,
    drag: Option<Drag>,
    /// Anchors per subpath, as of the last change this editor knows about.
    /// Anchor ids are positions, so they only keep pointing at the same
    /// anchors while this layout holds; see `revalidate`.
    layout: Vec<usize>,
}

impl PathEdit {
    /// Start editing `id`. `None` if it is not a vector node.
    pub fn begin(doc: &Document, id: NodeId) -> Result<Option<Self>> {
        if !matches!(doc.get(id)?.kind, NodeKind::Vector(_)) {
            return Ok(None);
        }
        Ok(Some(Self {
            id,
            selected: Vec::new(),
            drag: None,
            layout: layout_of(&AnchorPath::from_bez(doc.vector_path(id)?)),
        }))
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    pub fn selected(&self) -> &[AnchorId] {
        &self.selected
    }

    /// Whether a press at `point` would pick up a point or a handle, rather
    /// than land on a segment or miss — asked before the first press into a
    /// path, which should not insert an anchor where the path was grabbed.
    pub fn picks_point(&self, doc: &Document, point: Point, tolerance: f64) -> Result<bool> {
        let (to_local, scale) = self.local_space(doc)?;
        let anchors = AnchorPath::from_bez(doc.vector_path(self.id)?);
        Ok(matches!(
            anchors.hit(to_local * point, tolerance / scale, &self.selected),
            Some(PathHit::Anchor(_) | PathHit::Handle(..))
        ))
    }

    /// Whether a press is in progress, i.e. the document holds a preview.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn press(
        &mut self,
        doc: &mut Document,
        point: Point,
        tolerance: f64,
        additive: bool,
    ) -> Result<PressOutcome> {
        let (to_local, scale) = self.local_space(doc)?;
        let local = to_local * point;
        let original = doc.vector_path(self.id)?.clone();
        let mut anchors = AnchorPath::from_bez(&original);

        let (outcome, kind) = match anchors.hit(local, tolerance / scale, &self.selected) {
            Some(PathHit::Handle(anchor, side)) => {
                let smooth = anchors.get(anchor).is_some_and(|a| a.is_smooth());
                let kind = DragKind::Handle {
                    anchor,
                    side,
                    smooth,
                };
                (PressOutcome::Handle, Some(kind))
            }
            Some(PathHit::Anchor(id)) => {
                if additive {
                    toggle(&mut self.selected, id);
                } else if !self.selected.contains(&id) {
                    self.selected = vec![id];
                }
                // Shift-clicking an anchor off must not then drag the rest.
                let drags = self.selected.contains(&id);
                (PressOutcome::Anchor, drags.then_some(DragKind::Anchors))
            }
            Some(PathHit::Segment { from, t }) => {
                let inserted = anchors.split_segment(from, t);
                doc.write_path(self.id, anchors.to_bez())?;
                self.selected = vec![inserted];
                self.layout = layout_of(&anchors);
                (PressOutcome::Segment, Some(DragKind::Anchors))
            }
            None => {
                if !additive {
                    self.selected.clear();
                }
                (PressOutcome::Miss, None)
            }
        };
        self.drag = kind.map(|kind| Drag {
            original,
            anchors,
            start: local,
            kind,
        });
        Ok(outcome)
    }

    /// Follow the pointer. Shift constrains a move to one axis and a handle
    /// to 45° steps; Alt lets a smooth point's handles move independently.
    pub fn update(&mut self, doc: &mut Document, point: Point, modifiers: Modifiers) -> Result<()> {
        let (to_local, _) = self.local_space(doc)?;
        let Some(drag) = &self.drag else {
            return Ok(());
        };
        let local = to_local * point;
        let mut anchors = drag.anchors.clone();
        match drag.kind {
            DragKind::Anchors => {
                let mut delta = local - drag.start;
                if modifiers.shift {
                    if delta.x.abs() >= delta.y.abs() {
                        delta.y = 0.0;
                    } else {
                        delta.x = 0.0;
                    }
                }
                for &id in &self.selected {
                    if let Some(anchor) = anchors.get_mut(id) {
                        anchor.translate(delta);
                    }
                }
            }
            DragKind::Handle {
                anchor,
                side,
                smooth,
            } => {
                if let Some(a) = anchors.get_mut(anchor)
                    && let Some(handle) = a.handle(side)
                {
                    let mut moved = handle + (local - drag.start);
                    if modifiers.shift {
                        moved = constrain_45(a.point, moved);
                    }
                    a.set_handle(side, Some(moved));
                    // A smooth point stays smooth: the opposite handle swings
                    // round to stay in line, keeping its own length.
                    if smooth && !modifiers.alt {
                        let opposite = side.opposite();
                        if let Some(other) = a.handle(opposite) {
                            let reach = (other - a.point).hypot();
                            let direction = moved - a.point;
                            if direction.hypot() > 0.0 {
                                let swung = a.point - direction.normalize() * reach;
                                a.set_handle(opposite, Some(swung));
                            }
                        }
                    }
                }
            }
        }
        doc.write_path(self.id, anchors.to_bez())
    }

    /// End the drag, recording it as one undo step if it changed anything
    /// (an inserted anchor counts, even with no movement).
    pub fn release(&mut self, doc: &mut Document, journal: &mut Journal) -> Result<bool> {
        let Some(drag) = self.drag.take() else {
            return Ok(false);
        };
        let current = doc.vector_path(self.id)?.clone();
        if current == drag.original {
            return Ok(false);
        }
        doc.write_path(self.id, drag.original)?;
        journal.execute(
            doc,
            Command::SetPath {
                id: self.id,
                path: current,
            },
        )?;
        Ok(true)
    }

    /// Abandon the drag, restoring the path as it was at the press.
    pub fn cancel_drag(&mut self, doc: &mut Document) -> Result<()> {
        if let Some(drag) = self.drag.take() {
            // Cancelling a drag that inserted an anchor takes the anchor back
            // out, which shifts the ids after it.
            self.sync_layout(&AnchorPath::from_bez(&drag.original));
            doc.write_path(self.id, drag.original)?;
        }
        Ok(())
    }

    pub fn delete_selected(
        &mut self,
        doc: &mut Document,
        journal: &mut Journal,
    ) -> Result<DeleteOutcome> {
        if self.selected.is_empty() {
            return Ok(DeleteOutcome::Nothing);
        }
        let mut anchors = AnchorPath::from_bez(doc.vector_path(self.id)?);
        anchors.remove(&self.selected);
        self.selected.clear();
        self.layout = layout_of(&anchors);
        if anchors.is_empty() {
            return Ok(DeleteOutcome::EmptiedPath);
        }
        journal.execute(
            doc,
            Command::SetPath {
                id: self.id,
                path: anchors.to_bez(),
            },
        )?;
        Ok(DeleteOutcome::Anchors)
    }

    /// Move the selected anchors by a document-space offset, as one step.
    pub fn nudge(
        &mut self,
        doc: &mut Document,
        journal: &mut Journal,
        offset: Vec2,
    ) -> Result<bool> {
        if self.selected.is_empty() {
            return Ok(false);
        }
        let (to_local, _) = self.local_space(doc)?;
        // Offsets are vectors: map two points and take the difference, so
        // the node's translation drops out.
        let delta = to_local * Point::new(offset.x, offset.y) - to_local * Point::ORIGIN;
        let mut anchors = AnchorPath::from_bez(doc.vector_path(self.id)?);
        for &id in &self.selected {
            if let Some(anchor) = anchors.get_mut(id) {
                anchor.translate(delta);
            }
        }
        journal.execute(
            doc,
            Command::SetPath {
                id: self.id,
                path: anchors.to_bez(),
            },
        )?;
        Ok(true)
    }

    /// Double-click: toggle the anchor under `point` between corner and
    /// smooth. Returns false if there is no anchor there.
    pub fn toggle_smooth_at(
        &mut self,
        doc: &mut Document,
        journal: &mut Journal,
        point: Point,
        tolerance: f64,
    ) -> Result<bool> {
        let (to_local, scale) = self.local_space(doc)?;
        let mut anchors = AnchorPath::from_bez(doc.vector_path(self.id)?);
        let Some(PathHit::Anchor(id)) = anchors.hit(to_local * point, tolerance / scale, &[])
        else {
            return Ok(false);
        };
        anchors.toggle_smooth(id);
        journal.execute(
            doc,
            Command::SetPath {
                id: self.id,
                path: anchors.to_bez(),
            },
        )?;
        self.selected = vec![id];
        Ok(true)
    }

    /// After undo or redo may have changed the path under us. If anchors were
    /// added or removed, the selected ids now point at other anchors, so the
    /// selection is dropped; if they only moved, it is kept.
    pub fn revalidate(&mut self, doc: &Document) -> Result<()> {
        self.drag = None;
        self.sync_layout(&AnchorPath::from_bez(doc.vector_path(self.id)?));
        Ok(())
    }

    fn sync_layout(&mut self, anchors: &AnchorPath) {
        let layout = layout_of(anchors);
        if layout != self.layout {
            self.selected.clear();
            self.layout = layout;
        }
    }

    pub fn view(&self, doc: &Document) -> Result<EditView> {
        let world = doc.world_transform(self.id)?;
        let path = doc.vector_path(self.id)?;
        let anchors = AnchorPath::from_bez(path);
        let mut outline = path.clone();
        outline.apply_affine(world);
        let mut view = EditView {
            outline,
            ..Default::default()
        };
        for id in anchors.ids() {
            let anchor = anchors.get(id).expect("ids() yields valid ids");
            let selected = self.selected.contains(&id);
            view.anchors.push((world * anchor.point, selected));
            if selected {
                for handle in [anchor.handle_in, anchor.handle_out].into_iter().flatten() {
                    view.handles.push((world * anchor.point, world * handle));
                }
            }
        }
        Ok(view)
    }

    /// The document-to-local transform, and the factor that converts a
    /// document distance into a local one.
    fn local_space(&self, doc: &Document) -> Result<(Affine, f64)> {
        let world = doc.world_transform(self.id)?;
        let scale = world.determinant().abs().sqrt().max(1e-9);
        Ok((world.inverse(), scale))
    }
}

fn layout_of(anchors: &AnchorPath) -> Vec<usize> {
    anchors
        .subpaths
        .iter()
        .map(|sub| sub.anchors.len())
        .collect()
}

fn toggle(ids: &mut Vec<AnchorId>, id: AnchorId) {
    match ids.iter().position(|&s| s == id) {
        Some(index) => {
            ids.remove(index);
        }
        None => ids.push(id),
    }
}
