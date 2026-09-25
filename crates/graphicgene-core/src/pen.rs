//! The pen tool: drawing a new path one anchor at a time.
//!
//! Each press adds an anchor; dragging before release pulls out symmetric
//! handles, making it a smooth point. Pressing the first anchor closes the
//! path, pressing the last one ends it open. The path is attached while it is
//! drawn so it renders, but reaches the journal only when finished — so the
//! whole path is one undo step, and undo while drawing removes the last
//! anchor instead.

use crate::anchors::{Anchor, AnchorPath, HandleSide, Subpath, constrain_45};
use crate::command::{Command, Journal};
use crate::doc::Document;
use crate::error::Result;
use crate::geom::{BezPath, Point};
use crate::gesture::Modifiers;
use crate::node::{Node, NodeId, NodeKind, Stroke};
use crate::selection::Selection;

#[derive(Debug, Clone)]
pub struct PenSession {
    id: NodeId,
    path: AnchorPath,
    /// The current press landed on the first anchor: releasing closes.
    closing: bool,
    /// The current press landed on the last anchor: releasing ends.
    ending: bool,
    /// Where the pointer is between presses, for the rubber-band preview.
    pointer: Option<Point>,
    /// Whether that pointer is over the first anchor, i.e. a press closes.
    closable: bool,
    /// The selection before the pen started, restored on cancel.
    base: Vec<NodeId>,
}

impl PenSession {
    /// Start a path with its first anchor at `point`.
    pub fn start(
        doc: &mut Document,
        selection: &mut Selection,
        stroke: Stroke,
        point: Point,
    ) -> Result<Self> {
        let path = AnchorPath {
            subpaths: vec![Subpath {
                anchors: vec![Anchor::corner(point)],
                closed: false,
            }],
        };
        let mut node = Node::vector("Path", path.to_bez(), None);
        if let NodeKind::Vector(vector) = &mut node.kind {
            vector.stroke = Some(stroke);
        }
        let root = doc.root();
        let index = doc.children_of(root)?.len();
        let id = doc.insert_detached(node);
        doc.attach(id, root, index)?;
        let base = selection.ids().to_vec();
        selection.clear();
        Ok(Self {
            id,
            path,
            closing: false,
            ending: false,
            pointer: None,
            closable: false,
            base,
        })
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    pub fn anchors(&self) -> &[Anchor] {
        &self.path.subpaths[0].anchors
    }

    fn anchors_mut(&mut self) -> &mut Vec<Anchor> {
        &mut self.path.subpaths[0].anchors
    }

    /// A press after the first. `tolerance` is how close to the first or last
    /// anchor counts as pressing it, in document units.
    pub fn press(
        &mut self,
        doc: &mut Document,
        point: Point,
        modifiers: Modifiers,
        tolerance: f64,
    ) -> Result<()> {
        let anchors = self.anchors();
        let near = |p: Point| (p - point).hypot() <= tolerance;
        let (first, last) = (anchors[0].point, anchors[anchors.len() - 1].point);
        if anchors.len() >= 2 && near(first) {
            self.closing = true;
            self.path.subpaths[0].closed = true;
        } else if near(last) {
            // Pressing the last anchor ends the path. With only one anchor
            // so far there is nothing to end, and a second anchor on top of
            // the first would be a zero-length segment: ignore the press.
            self.ending = anchors.len() >= 2;
            return Ok(());
        } else {
            let point = if modifiers.shift {
                constrain_45(last, point)
            } else {
                point
            };
            self.anchors_mut().push(Anchor::corner(point));
        }
        self.closable = false;
        self.write(doc)
    }

    /// Dragging with the button down pulls out the handles of the anchor just
    /// placed (or of the first anchor, when closing): the outgoing handle
    /// follows the pointer, the incoming one mirrors it.
    pub fn drag(&mut self, doc: &mut Document, point: Point, modifiers: Modifiers) -> Result<()> {
        if self.ending {
            return Ok(());
        }
        let closing = self.closing;
        let anchors = self.anchors_mut();
        let anchor = if closing {
            &mut anchors[0]
        } else {
            anchors.last_mut().expect("a session always has an anchor")
        };
        let handle = if modifiers.shift {
            constrain_45(anchor.point, point)
        } else {
            point
        };
        let mirrored = anchor.point + (anchor.point - handle);
        anchor.set_handle(HandleSide::Out, Some(handle));
        anchor.set_handle(HandleSide::In, Some(mirrored));
        self.write(doc)
    }

    /// End of a press. Returns true when the path is complete — closed, or
    /// ended on its last anchor — and should be finished.
    pub fn release(&self) -> bool {
        self.closing || self.ending
    }

    /// Track the pointer between presses.
    pub fn hover(&mut self, point: Point, tolerance: f64) {
        self.pointer = Some(point);
        let anchors = self.anchors();
        self.closable = anchors.len() >= 2 && (anchors[0].point - point).hypot() <= tolerance;
    }

    /// Remove the last anchor. Returns false when none are left, in which
    /// case the caller should cancel the session.
    pub fn undo_anchor(&mut self, doc: &mut Document) -> Result<bool> {
        self.anchors_mut().pop();
        if self.anchors().is_empty() {
            return Ok(false);
        }
        self.write(doc)?;
        Ok(true)
    }

    /// Commit the path as one journal entry and select it. A path with fewer
    /// than two anchors is not a path: it is discarded and `None` returned.
    pub fn finish(
        self,
        doc: &mut Document,
        journal: &mut Journal,
        selection: &mut Selection,
    ) -> Result<Option<NodeId>> {
        if self.anchors().len() < 2 {
            self.cancel(doc, selection)?;
            return Ok(None);
        }
        let id = self.id;
        let (parent, index) = doc.detach(id)?;
        journal.execute(doc, Command::Attach { id, parent, index })?;
        selection.set([id]);
        Ok(Some(id))
    }

    /// Throw the path away.
    pub fn cancel(self, doc: &mut Document, selection: &mut Selection) -> Result<()> {
        // Left detached in the arena; save purges unreachable nodes.
        doc.detach(self.id)?;
        selection.set(self.base);
        Ok(())
    }

    /// The segment the next press would add, from the last anchor to the
    /// pointer. `None` while nothing is being aimed at.
    pub fn preview(&self) -> Option<BezPath> {
        let pointer = self.pointer?;
        if self.closing || self.ending {
            return None;
        }
        let last = self.anchors().last()?;
        let mut path = BezPath::new();
        path.move_to(last.point);
        match last.handle_out {
            Some(out) => path.curve_to(out, pointer, pointer),
            None => path.line_to(pointer),
        }
        Some(path)
    }

    /// Whether a press at the current pointer would close the path.
    pub fn closable(&self) -> bool {
        self.closable
    }

    fn write(&self, doc: &mut Document) -> Result<()> {
        doc.write_path(self.id, self.path.to_bez())
    }
}
