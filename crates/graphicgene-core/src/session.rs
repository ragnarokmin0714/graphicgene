//! An editing session: one document, and everything the user is doing to it.
//!
//! The document and its undo journal, the selection, the node under the
//! pointer, and whichever interaction is in progress — a drag, a pen path, a
//! path being edited — together with the rules that tie them together: undo
//! while drawing removes a pen anchor, Delete means whatever the current mode
//! has selected, Escape and Enter leave a mode, undo prunes the selection.
//!
//! This is what a platform shell drives: the wasm bindings today, a desktop
//! app later. It takes typed arguments in document space. Turning pointer
//! events into document points, ids into strings and results into JSON is
//! the shell's job, and none of these rules may live there — a rule written
//! twice is a rule the two apps will disagree on.
//!
//! Tolerances are passed in rather than fixed here: they are screen
//! distances divided by the zoom, which only the shell knows.

use std::fmt;

use crate::color::LinearRgba;
use crate::command::{Command, Journal};
use crate::doc::{Changes, Document};
use crate::error::Result;
use crate::geom::{Affine, BezPath, Point, Rect, Size, Vec2};
use crate::gesture::{self, Frame, Gesture, Modifiers, ShapeKind, TransformKind};
use crate::hit;
use crate::layout;
use crate::node::{Node, NodeId, NodeKind, Stroke};
use crate::path_edit::{DeleteOutcome, EditView, PathEdit, PressOutcome};
use crate::pen::PenSession;
use crate::project::Project;
use crate::selection::Selection;

/// An interaction that outlives a single press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A path is being drawn with the pen.
    Pen,
    /// A path's anchors are being edited.
    PathEdit,
}

/// What a select-tool press should turn into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectOutcome {
    /// It landed on a node that is now selected: follow with a move.
    Drag,
    /// It landed on a node, but Shift deselected it: do nothing. Kept apart
    /// from `Miss` because a marquee started on the node just deselected
    /// would pick it straight back up on the first jitter.
    Hit,
    /// It landed on empty space: follow with a marquee.
    Miss,
}

/// One row of the layer panel.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerRow {
    pub id: NodeId,
    /// 0 for the root's children.
    pub depth: u32,
    pub name: String,
    pub is_group: bool,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
    pub selected: bool,
}

/// Changes whenever the layer rows may have changed; see
/// [`Session::layers_version`]. Compare for equality only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayersVersion {
    generation: u64,
    structure: u64,
    journal: u64,
    selection: u64,
}

impl fmt::Display for LayersVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.generation, self.structure, self.journal, self.selection
        )
    }
}

/// The pen's path in progress, for the overlay. Document space.
#[derive(Debug, Clone, Default)]
pub struct PenView {
    pub anchors: Vec<Point>,
    /// (anchor, handle) pairs of the anchor being placed.
    pub handles: Vec<(Point, Point)>,
    /// From the last anchor to the pointer.
    pub preview: Option<BezPath>,
    /// A press now would close the path on its first anchor.
    pub closable: bool,
}

/// Everything drawn over the artwork, in document space.
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    pub mode: Option<Mode>,
    /// The selection frame, following a transform while it is dragged.
    pub frame: Option<Frame>,
    /// Outlines of the selected nodes.
    pub outlines: Vec<BezPath>,
    /// Outline of the node under the pointer, unless it is selected.
    pub hover: Option<BezPath>,
    pub marquee: Option<Rect>,
    /// What the drag in progress does: "move", "scale", "rotate", "create"
    /// or "marquee".
    pub gesture: Option<&'static str>,
    pub pen: Option<PenView>,
    pub path: Option<EditView>,
}

#[derive(Debug, Default)]
pub struct Session {
    document: Document,
    journal: Journal,
    selection: Selection,
    /// The drag in progress. At most one at a time.
    gesture: Option<Gesture>,
    /// The node under the pointer, for the hover outline.
    hover: Option<NodeId>,
    /// A path being drawn with the pen. Never together with `path_edit`.
    pen: Option<PenSession>,
    /// The path whose anchors are being edited.
    path_edit: Option<PathEdit>,
    /// Bumped whenever a different document is loaded.
    generation: u64,
}

impl Session {
    /// A session on a new document with the default artboard.
    pub fn new() -> Self {
        Self::default()
    }

    /// A session on a new document with an artboard of the given size.
    pub fn with_artboard(artboard: Size) -> Self {
        Self {
            document: Document::with_artboard(artboard),
            ..Self::default()
        }
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    // ---- Document and history ------------------------------------------------

    /// Add a node at the top of the root, as one undo step.
    pub fn insert(&mut self, node: Node) -> Result<NodeId> {
        let parent = self.document.root();
        let index = self.document.children_of(parent)?.len();
        self.journal.execute(
            &mut self.document,
            Command::InsertNode {
                parent,
                index,
                node,
            },
        )?;
        Ok(self.document.children_of(parent)?[index])
    }

    /// Apply a command as one undo step.
    pub fn execute(&mut self, command: Command) -> Result<()> {
        self.journal.execute(&mut self.document, command)
    }

    /// Undo. While the pen is drawing, this removes the last anchor instead:
    /// the unfinished path is not in the journal yet.
    pub fn undo(&mut self) -> Result<bool> {
        self.cancel_gesture()?;
        if let Some(pen) = self.pen.as_mut() {
            if !pen.undo_anchor(&mut self.document)? {
                let pen = self.pen.take().expect("checked above");
                pen.cancel(&mut self.document, &mut self.selection)?;
            }
            return Ok(true);
        }
        self.cancel_path_drag()?;
        let changed = self.journal.undo(&mut self.document)?;
        self.after_history_change(changed)?;
        Ok(changed)
    }

    pub fn redo(&mut self) -> Result<bool> {
        if self.pen.is_some() {
            return Ok(false);
        }
        self.cancel_gesture()?;
        self.cancel_path_drag()?;
        let changed = self.journal.redo(&mut self.document)?;
        self.after_history_change(changed)?;
        Ok(changed)
    }

    pub fn can_undo(&self) -> bool {
        self.pen.is_some() || self.journal.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.pen.is_none() && self.journal.can_redo()
    }

    /// Whether a press is in progress, so the document holds a preview that
    /// should not be saved yet: a drag, a pen path, or a path-edit drag.
    pub fn busy(&self) -> bool {
        self.gesture.is_some()
            || self.pen.is_some()
            || self.path_edit.as_ref().is_some_and(PathEdit::is_dragging)
    }

    /// The project file. The copy written drops detached nodes: the live
    /// document keeps them because undo depends on them, but a file has no
    /// undo history, so saving them would only make each save larger.
    pub fn save(&self) -> Result<String> {
        let mut document = self.document.clone();
        document.purge_unreachable();
        Project::new(document).to_json()
    }

    /// Replace the document with a project file, ending any interaction and
    /// forgetting the undo history. On error nothing changes.
    pub fn load(&mut self, text: &str) -> Result<()> {
        let project = Project::from_json(text)?;
        self.gesture = None;
        self.pen = None;
        self.path_edit = None;
        self.hover = None;
        self.document = project.document;
        self.journal.clear();
        self.selection.clear();
        self.generation += 1;
        Ok(())
    }

    /// The artwork as SVG, the size of the artboard.
    pub fn export_svg(&self) -> Result<String> {
        crate::svg::to_svg(&self.document)
    }

    /// Run the layout pass and hand over what changed since the last call:
    /// the start of every frame's render.
    pub fn prepare_render(&mut self) -> Result<Changes> {
        layout::run(&mut self.document)?;
        Ok(self.document.take_changes())
    }

    // ---- Layer panel -----------------------------------------------------------

    /// Changes whenever the layer rows may have, so a view can cache them.
    ///
    /// Rows only change through the journal, the tree's shape, the selection
    /// or a load — never through a drag's preview, which writes transforms and
    /// paths only. So the version holds still for a whole drag and the panel
    /// is not rebuilt on every frame of one.
    pub fn layers_version(&self) -> LayersVersion {
        LayersVersion {
            generation: self.generation,
            structure: self.document.structure_version(),
            journal: self.journal.ops(),
            selection: self.selection.version(),
        }
    }

    /// The layer tree in panel order, not paint order: the root is omitted,
    /// and the topmost sibling comes first, with each group directly above
    /// its own children.
    pub fn layer_rows(&self) -> Result<Vec<LayerRow>> {
        let mut rows = Vec::new();
        // Children are stored bottom-to-top, so pushing them in stored order
        // pops the topmost one first.
        let mut stack: Vec<(NodeId, u32)> = self
            .document
            .children_of(self.document.root())?
            .iter()
            .map(|&id| (id, 0))
            .collect();
        while let Some((id, depth)) = stack.pop() {
            let node = self.document.get(id)?;
            if let Some(children) = node.children() {
                stack.extend(children.iter().map(|&child| (child, depth + 1)));
            }
            rows.push(LayerRow {
                id,
                depth,
                name: node.common.name.clone(),
                is_group: matches!(node.kind, NodeKind::Group(_)),
                visible: node.common.visible,
                locked: node.common.locked,
                opacity: node.common.opacity,
                selected: self.selection.contains(id),
            });
        }
        Ok(rows)
    }

    // ---- Selection ---------------------------------------------------------------

    /// A click with the select tool.
    pub fn select_at(
        &mut self,
        point: Point,
        additive: bool,
        tolerance: f64,
    ) -> Result<SelectOutcome> {
        let hit = hit::hit_test(&self.document, point, tolerance)?;
        Ok(match (hit, self.selection.click(hit, additive)) {
            (_, true) => SelectOutcome::Drag,
            (Some(_), false) => SelectOutcome::Hit,
            (None, false) => SelectOutcome::Miss,
        })
    }

    /// A click on a layer-panel row. Ends the pen or path editing.
    pub fn select_layer(&mut self, id: NodeId, additive: bool) -> Result<()> {
        self.document.get(id)?;
        self.finish_mode()?;
        if additive {
            self.selection.toggle(id);
        } else {
            self.selection.set([id]);
        }
        Ok(())
    }

    pub fn select_all(&mut self) -> Result<()> {
        self.finish_mode()?;
        let ids = hit::selectable(&self.document)?;
        self.selection.set(ids);
        Ok(())
    }

    pub fn clear_selection(&mut self) -> Result<()> {
        self.finish_mode()?;
        self.selection.clear();
        Ok(())
    }

    /// Track the node under the pointer. Returns whether that changed, so a
    /// view only redraws the overlay when it has to.
    pub fn hover(&mut self, point: Point, tolerance: f64) -> Result<bool> {
        let hit = hit::hit_test(&self.document, point, tolerance)?;
        let changed = hit != self.hover;
        self.hover = hit;
        Ok(changed)
    }

    pub fn clear_hover(&mut self) -> bool {
        self.hover.take().is_some()
    }

    /// Delete, as one undo step, whatever the current mode has selected: the
    /// last pen anchor while drawing, the selected anchors while editing a
    /// path, otherwise the selected nodes.
    pub fn delete_selection(&mut self) -> Result<bool> {
        if self.pen.is_some() {
            return self.undo();
        }
        if let Some(edit) = self.path_edit.as_mut() {
            match edit.delete_selected(&mut self.document, &mut self.journal)? {
                DeleteOutcome::Nothing => return Ok(false),
                DeleteOutcome::Anchors => {}
                DeleteOutcome::EmptiedPath => {
                    let id = edit.id();
                    self.path_edit = None;
                    self.execute(Command::Detach { id })?;
                    self.selection.clear();
                }
            }
            return Ok(true);
        }
        if self.selection.is_empty() {
            return Ok(false);
        }
        let commands = self
            .selection
            .ids()
            .iter()
            .map(|&id| Command::Detach { id })
            .collect();
        self.execute(Command::Batch(commands))?;
        self.selection.clear();
        self.hover = None;
        Ok(true)
    }

    /// Move the selection — or the selected anchors, while editing a path —
    /// by a document-space offset, as one undo step.
    pub fn nudge(&mut self, offset: Vec2) -> Result<bool> {
        if self.pen.is_some() || self.gesture.is_some() {
            return Ok(false);
        }
        if let Some(edit) = self.path_edit.as_mut() {
            return edit.nudge(&mut self.document, &mut self.journal, offset);
        }
        if self.selection.is_empty() {
            return Ok(false);
        }
        let command = gesture::world_delta_command(
            &self.document,
            self.selection.ids(),
            Affine::translate(offset),
        )?;
        self.execute(command)?;
        Ok(true)
    }

    // ---- Gestures ------------------------------------------------------------------
    //
    // One press-drag-release: begin_* on press, update_gesture on every move,
    // end_gesture on release, cancel_gesture on Escape or a lost pointer. The
    // drag previews in the document and lands in the journal as one step.

    /// Start moving, scaling or rotating the selection. Returns false if the
    /// selection has nothing to manipulate.
    pub fn begin_transform(&mut self, kind: TransformKind, point: Point) -> Result<bool> {
        self.cancel_gesture()?;
        self.gesture = Gesture::transform(&self.document, &self.selection, kind, point)?;
        Ok(self.gesture.is_some())
    }

    /// Start drawing a shape.
    pub fn begin_create(&mut self, shape: ShapeKind, fill: LinearRgba, point: Point) -> Result<()> {
        self.cancel_gesture()?;
        let gesture = Gesture::create(&mut self.document, &mut self.selection, shape, fill, point)?;
        self.gesture = Some(gesture);
        Ok(())
    }

    pub fn begin_marquee(&mut self, point: Point, additive: bool) -> Result<()> {
        self.cancel_gesture()?;
        self.gesture = Some(Gesture::marquee(&self.selection, point, additive));
        Ok(())
    }

    /// Follow the pointer. A no-op when no gesture is active.
    pub fn update_gesture(&mut self, point: Point, modifiers: Modifiers) -> Result<()> {
        if let Some(gesture) = self.gesture.as_mut() {
            gesture.update(&mut self.document, &mut self.selection, point, modifiers)?;
        }
        Ok(())
    }

    /// Finish the gesture. Returns the new node when a shape was drawn.
    pub fn end_gesture(&mut self) -> Result<Option<NodeId>> {
        match self.gesture.take() {
            Some(gesture) => {
                gesture.commit(&mut self.document, &mut self.journal, &mut self.selection)
            }
            None => Ok(None),
        }
    }

    /// Abandon the gesture. Returns whether one was active.
    pub fn cancel_gesture(&mut self) -> Result<bool> {
        let Some(gesture) = self.gesture.take() else {
            return Ok(false);
        };
        gesture.cancel(&mut self.document, &mut self.selection)?;
        Ok(true)
    }

    // ---- Pen ---------------------------------------------------------------------

    /// A press with the pen tool. The first press starts a new path, stroked
    /// with `stroke`; the others add anchors. `tolerance` is how near an end
    /// anchor counts as pressing it.
    pub fn pen_press(
        &mut self,
        point: Point,
        shift: bool,
        tolerance: f64,
        stroke: Stroke,
    ) -> Result<()> {
        let modifiers = Modifiers { shift, alt: false };
        match self.pen.as_mut() {
            Some(pen) => pen.press(&mut self.document, point, modifiers, tolerance)?,
            None => {
                self.cancel_gesture()?;
                self.end_path_edit()?;
                let pen =
                    PenSession::start(&mut self.document, &mut self.selection, stroke, point)?;
                self.pen = Some(pen);
            }
        }
        self.hover = None;
        Ok(())
    }

    pub fn pen_drag(&mut self, point: Point, shift: bool) -> Result<()> {
        if let Some(pen) = self.pen.as_mut() {
            pen.drag(&mut self.document, point, Modifiers { shift, alt: false })?;
        }
        Ok(())
    }

    /// End of a pen press. When that completed the path (it closed, or ended
    /// on its last anchor), the path is finished and returned.
    pub fn pen_release(&mut self) -> Result<Option<NodeId>> {
        match &self.pen {
            Some(pen) if pen.release() => self.pen_finish(),
            _ => Ok(None),
        }
    }

    /// Track the pointer between pen presses. Returns whether a path is being
    /// drawn, i.e. whether the overlay's preview moved.
    pub fn pen_hover(&mut self, point: Point, tolerance: f64) -> bool {
        match self.pen.as_mut() {
            Some(pen) => {
                pen.hover(point, tolerance);
                true
            }
            None => false,
        }
    }

    /// Finish the path being drawn. Returns it, or nothing if there was no
    /// path or it was too short to keep.
    pub fn pen_finish(&mut self) -> Result<Option<NodeId>> {
        match self.pen.take() {
            Some(pen) => pen.finish(&mut self.document, &mut self.journal, &mut self.selection),
            None => Ok(None),
        }
    }

    /// Leave whichever mode is active: finish the pen path, or stop editing
    /// a path. Returns whether there was one — what Enter and Escape check
    /// before falling back to their other meanings.
    pub fn finish_mode(&mut self) -> Result<bool> {
        if self.pen.is_some() {
            self.pen_finish()?;
            return Ok(true);
        }
        if self.path_edit.is_some() {
            self.end_path_edit()?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn mode(&self) -> Option<Mode> {
        if self.pen.is_some() {
            Some(Mode::Pen)
        } else if self.path_edit.is_some() {
            Some(Mode::PathEdit)
        } else {
            None
        }
    }

    // ---- Path editing ----------------------------------------------------------

    /// Start editing the anchors of the selected path. Returns false unless
    /// exactly one vector node is selected.
    pub fn begin_path_edit(&mut self) -> Result<bool> {
        if self.pen.is_some() {
            return Ok(false);
        }
        let &[id] = self.selection.ids() else {
            return Ok(false);
        };
        self.cancel_gesture()?;
        self.path_edit = PathEdit::begin(&self.document, id)?;
        self.hover = None;
        Ok(self.path_edit.is_some())
    }

    /// A press while editing a path. `Miss` when not editing.
    pub fn path_press(
        &mut self,
        point: Point,
        tolerance: f64,
        additive: bool,
    ) -> Result<PressOutcome> {
        match self.path_edit.as_mut() {
            Some(edit) => edit.press(&mut self.document, point, tolerance, additive),
            None => Ok(PressOutcome::Miss),
        }
    }

    pub fn path_drag(&mut self, point: Point, modifiers: Modifiers) -> Result<()> {
        if let Some(edit) = self.path_edit.as_mut() {
            edit.update(&mut self.document, point, modifiers)?;
        }
        Ok(())
    }

    /// End a path-edit drag. Returns whether it changed the path.
    pub fn path_release(&mut self) -> Result<bool> {
        match self.path_edit.as_mut() {
            Some(edit) => edit.release(&mut self.document, &mut self.journal),
            None => Ok(false),
        }
    }

    pub fn path_cancel_drag(&mut self) -> Result<()> {
        self.cancel_path_drag()
    }

    /// A double-click while editing: on an anchor it toggles corner/smooth;
    /// on the path it does nothing; off the path it stops editing. Returns
    /// whether editing continues.
    pub fn path_double_click(&mut self, point: Point, tolerance: f64) -> Result<bool> {
        let Some(edit) = self.path_edit.as_mut() else {
            return Ok(false);
        };
        if edit.toggle_smooth_at(&mut self.document, &mut self.journal, point, tolerance)? {
            return Ok(true);
        }
        let id = edit.id();
        if hit::hit_test(&self.document, point, tolerance)? == Some(id) {
            return Ok(true);
        }
        self.end_path_edit()?;
        Ok(false)
    }

    // ---- Overlay -------------------------------------------------------------------

    /// Everything drawn over the artwork: in path editing and pen modes their
    /// points and handles, otherwise the selection frame, outlines, the hover
    /// outline and the marquee. Handle *sizes* are left to the view, since
    /// they are screen measurements.
    pub fn overlay(&self) -> Result<Overlay> {
        if let Some(pen) = &self.pen {
            let anchors = pen.anchors();
            // Handles of the anchor being placed, so a drag shows what it pulls.
            let handles = anchors
                .last()
                .map(|a| {
                    [a.handle_in, a.handle_out]
                        .into_iter()
                        .flatten()
                        .map(|h| (a.point, h))
                        .collect()
                })
                .unwrap_or_default();
            return Ok(Overlay {
                mode: Some(Mode::Pen),
                pen: Some(PenView {
                    anchors: anchors.iter().map(|a| a.point).collect(),
                    handles,
                    preview: pen.preview(),
                    closable: pen.closable(),
                }),
                ..Overlay::default()
            });
        }
        if let Some(edit) = &self.path_edit {
            return Ok(Overlay {
                mode: Some(Mode::PathEdit),
                path: Some(edit.view(&self.document)?),
                ..Overlay::default()
            });
        }

        let frame = match self.gesture.as_ref().and_then(Gesture::frame) {
            Some(frame) => Some(frame),
            None => Frame::of(&self.document, self.selection.ids())?,
        };
        let mut outlines = Vec::with_capacity(self.selection.len());
        for &id in self.selection.ids() {
            if let Some(outline) = self.outline(id)? {
                outlines.push(outline);
            }
        }
        let hover = match self.hover {
            Some(id) if !self.selection.contains(id) && self.document.is_attached(id) => {
                self.outline(id)?
            }
            _ => None,
        };
        Ok(Overlay {
            mode: None,
            frame,
            outlines,
            hover,
            marquee: self.gesture.as_ref().and_then(Gesture::marquee_rect),
            gesture: self.gesture.as_ref().map(Gesture::label),
            pen: None,
            path: None,
        })
    }

    // ---- Internals -----------------------------------------------------------------

    /// After undo or redo: drop selected, hovered and edited nodes that the
    /// history change took out of the tree.
    fn after_history_change(&mut self, changed: bool) -> Result<()> {
        if !changed {
            return Ok(());
        }
        self.selection.retain_attached(&self.document);
        if self.hover.is_some_and(|id| !self.document.is_attached(id)) {
            self.hover = None;
        }
        if let Some(edit) = self.path_edit.as_mut() {
            if self.document.is_attached(edit.id()) {
                edit.revalidate(&self.document)?;
            } else {
                self.path_edit = None;
            }
        }
        Ok(())
    }

    fn cancel_path_drag(&mut self) -> Result<()> {
        if let Some(edit) = self.path_edit.as_mut() {
            edit.cancel_drag(&mut self.document)?;
        }
        Ok(())
    }

    fn end_path_edit(&mut self) -> Result<()> {
        self.cancel_path_drag()?;
        self.path_edit = None;
        Ok(())
    }

    /// A node's outline in document space.
    fn outline(&self, id: NodeId) -> Result<Option<BezPath>> {
        let NodeKind::Vector(vector) = &self.document.get(id)?.kind else {
            return Ok(None);
        };
        let mut path = vector.path.clone();
        path.apply_affine(self.document.world_transform(id)?);
        Ok(Some(path))
    }
}
