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
//!
//! One interaction at a time: starting anything else — a press, undo, a new
//! selection — first abandons a property edit left open, as it does a drag.

mod text;
mod tools;

use std::fmt;

use crate::align::{self, Align, Distribute};
use crate::appearance::{self, Appearance};
use crate::clipboard;
use crate::color::LinearRgba;
use crate::command::{Command, Journal};
use crate::doc::{Changes, Document};
use crate::error::Result;
use crate::fonts::Fonts;
use crate::geom::{Affine, BezPath, Point, Rect, Size, Vec2};
use crate::gesture::{self, Frame, Gesture, Modifiers, ShapeKind, TransformKind};
use crate::hit;
use crate::layers::{self, Arrange, Drop};
use crate::layout;
use crate::node::{Node, NodeId, NodeKind, Stroke};
use crate::path_edit::{DeleteOutcome, EditView, PathEdit, PressOutcome};
use crate::pen::PenSession;
use crate::project::Project;
use crate::properties::{self, Properties, Property, PropertyEdit};
use crate::selection::Selection;
use crate::snap::Guide;

use text::TextEdit;
pub use text::TextView;
use tools::Route;
pub use tools::{Grab, PEN_STROKE_WIDTH, Pointer, Tool};

/// An interaction that outlives a single press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A path is being drawn with the pen.
    Pen,
    /// A path's anchors are being edited.
    PathEdit,
    /// Text is being typed, in the shell's text field.
    Text,
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

/// What a layer row shows as its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Group,
    Vector,
    Text,
}

/// One row of the layer panel.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerRow {
    pub id: NodeId,
    /// 0 for the root's children.
    pub depth: u32,
    pub name: String,
    pub kind: LayerKind,
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
    /// Something selected is locked, itself or through a group: the frame
    /// shows where it is, but the canvas cannot move it.
    pub locked: bool,
    /// What the drag in progress does: "move", "scale", "rotate", "create"
    /// or "marquee".
    pub gesture: Option<&'static str>,
    /// The lines the drag in progress snapped to, one per axis at most.
    pub guides: [Option<Guide>; 2],
    pub pen: Option<PenView>,
    pub path: Option<EditView>,
    pub text: Option<TextView>,
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
    /// A properties-panel change being previewed. Never together with a
    /// gesture or the pen.
    property_edit: Option<PropertyEdit>,
    /// What the pointer does on the canvas.
    tool: Tool,
    /// Where the moves and release of the press in progress go.
    route: Option<Route>,
    /// Where the last press went, so the double-click that ends a pen path
    /// is not taken as one that edits it.
    last_route: Option<Route>,
    /// Text being typed.
    text_edit: Option<TextEdit>,
    /// What text is set in. Not document state: a document names families,
    /// and whoever opens it supplies them.
    fonts: Fonts,
    /// The fonts' version the last layout pass ran with.
    fonts_seen: u64,
    /// Bumped when the layout pass lays text out; see `glyphs_version`.
    glyphs_version: u64,
    /// Bumped whenever a different document is loaded.
    generation: u64,
    /// Snapping is on unless this is set; a viewer's choice, not saved.
    snapping_off: bool,
    /// How near counts as snapping, in document units: the last pointer
    /// event's pick distance, which follows the zoom.
    snap_tolerance: f64,
    /// Ctrl was held at the last pointer event: no snapping for now.
    snap_held_off: bool,
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
        self.cancel_property()?;
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
        self.cancel_property()?;
        self.journal.execute(&mut self.document, command)
    }

    /// Undo. While the pen is drawing, this removes the last anchor instead:
    /// the unfinished path is not in the journal yet.
    pub fn undo(&mut self) -> Result<bool> {
        self.cancel_gesture()?;
        self.cancel_property()?;
        self.commit_text()?;
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
        self.cancel_property()?;
        self.commit_text()?;
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

    /// Whether the document holds a preview that should not be saved yet: a
    /// drag, a pen path, a path-edit drag or a property edit in progress.
    ///
    /// Text being typed is not one: typing can last minutes, and what has
    /// been typed is worth keeping if the tab closes before it is committed.
    pub fn busy(&self) -> bool {
        self.gesture.is_some()
            || self.pen.is_some()
            || self.path_edit.as_ref().is_some_and(PathEdit::is_dragging)
            || self.property_edit.is_some()
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
        self.property_edit = None;
        self.route = None;
        self.text_edit = None;
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
        self.layout()?;
        Ok(self.document.take_changes())
    }

    /// Run the layout pass, so what it works out — text boxes above all —
    /// is current. Costs nothing when nothing changed, so anything a view
    /// reads that depends on layout (properties, the overlay) runs it first
    /// rather than wait for the next frame's render.
    pub fn layout(&mut self) -> Result<()> {
        if layout::run(&mut self.document, &self.fonts, &mut self.fonts_seen)? {
            self.glyphs_version += 1;
        }
        Ok(())
    }

    // ---- Layer panel -----------------------------------------------------------

    /// Changes whenever the layer rows may have, so a view can cache them.
    ///
    /// Rows only change through the journal, the tree's shape, the selection
    /// or a load — never through a drag's preview, which writes transforms and
    /// paths only. So the version holds still for a whole drag and the panel
    /// is not rebuilt on every frame of one. A property preview can change a
    /// row's opacity; the rows show it once the edit is committed.
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
                kind: match node.kind {
                    NodeKind::Group(_) => LayerKind::Group,
                    NodeKind::Vector(_) => LayerKind::Vector,
                    NodeKind::Text(_) => LayerKind::Text,
                },
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
        self.cancel_property()?;
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
        self.cancel_property()?;
        self.finish_mode()?;
        if additive {
            self.selection.toggle(id);
        } else {
            self.selection.set([id]);
        }
        Ok(())
    }

    pub fn select_all(&mut self) -> Result<()> {
        self.cancel_property()?;
        self.finish_mode()?;
        let ids = hit::selectable(&self.document)?;
        self.selection.set(ids);
        Ok(())
    }

    pub fn clear_selection(&mut self) -> Result<()> {
        self.cancel_property()?;
        self.finish_mode()?;
        self.selection.clear();
        Ok(())
    }

    /// Track the node under the pointer — the one a press would select, so
    /// with the direct selection tool, inside groups. Returns whether that
    /// changed, so a view only redraws the overlay when it has to.
    pub fn hover(&mut self, point: Point, tolerance: f64) -> Result<bool> {
        let hit = if self.tool == Tool::Direct {
            hit::hit_test_deep(&self.document, point, tolerance)?
        } else {
            hit::hit_test(&self.document, point, tolerance)?
        };
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
        self.cancel_property()?;
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
        self.cancel_property()?;
        if let Some(edit) = self.path_edit.as_mut() {
            return edit.nudge(&mut self.document, &mut self.journal, offset);
        }
        if self.selection.is_empty() || self.selection_locked()? {
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
    /// selection has nothing to manipulate, or something in it is locked.
    pub fn begin_transform(&mut self, kind: TransformKind, point: Point) -> Result<bool> {
        self.cancel_gesture()?;
        self.cancel_property()?;
        if self.selection_locked()? {
            return Ok(false);
        }
        self.gesture = Gesture::transform(
            &self.document,
            &self.selection,
            kind,
            point,
            self.snapping(),
        )?;
        Ok(self.gesture.is_some())
    }

    /// Start drawing a shape.
    pub fn begin_create(&mut self, shape: ShapeKind, fill: LinearRgba, point: Point) -> Result<()> {
        self.cancel_gesture()?;
        self.cancel_property()?;
        let snap = (self.snapping() && !self.snap_held_off).then_some(self.snap_tolerance);
        let gesture = Gesture::create(
            &mut self.document,
            &mut self.selection,
            shape,
            fill,
            point,
            snap,
        )?;
        self.gesture = Some(gesture);
        Ok(())
    }

    pub fn begin_marquee(&mut self, point: Point, additive: bool) -> Result<()> {
        self.cancel_gesture()?;
        self.cancel_property()?;
        self.gesture = Some(Gesture::marquee(&self.selection, point, additive));
        Ok(())
    }

    /// Follow the pointer. A no-op when no gesture is active.
    pub fn update_gesture(&mut self, point: Point, modifiers: Modifiers) -> Result<()> {
        if let Some(gesture) = self.gesture.as_mut() {
            gesture.update(
                &mut self.document,
                &mut self.selection,
                point,
                modifiers,
                self.snap_tolerance,
            )?;
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
        let modifiers = Modifiers {
            shift,
            ..Modifiers::default()
        };
        match self.pen.as_mut() {
            Some(pen) => pen.press(&mut self.document, point, modifiers, tolerance)?,
            None => {
                self.cancel_gesture()?;
                self.cancel_property()?;
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
            pen.drag(
                &mut self.document,
                point,
                Modifiers {
                    shift,
                    ..Modifiers::default()
                },
            )?;
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
        if self.editing_text() {
            self.commit_text()?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn mode(&self) -> Option<Mode> {
        if self.pen.is_some() {
            Some(Mode::Pen)
        } else if self.path_edit.is_some() {
            Some(Mode::PathEdit)
        } else if self.text_edit.is_some() {
            Some(Mode::Text)
        } else {
            None
        }
    }

    // ---- Path editing ----------------------------------------------------------

    /// Start editing the anchors of the selected path. Returns false unless
    /// exactly one vector node is selected, and it is not locked.
    pub fn begin_path_edit(&mut self) -> Result<bool> {
        if self.pen.is_some() || self.selection_locked()? {
            return Ok(false);
        }
        let &[id] = self.selection.ids() else {
            return Ok(false);
        };
        self.cancel_gesture()?;
        self.cancel_property()?;
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
        self.cancel_property()?;
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
        self.cancel_property()?;
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

    // ---- Properties panel ------------------------------------------------------------
    //
    // A slider or a scrubbed number previews on every move and commits once
    // on release; a typed value or a picked swatch is set in one call. Either
    // way it is one undo step, and Escape restores what was there.

    /// What the properties panel shows for the selection. `None` with
    /// nothing selected, and while the pen is drawing.
    pub fn properties(&self) -> Result<Option<Properties>> {
        if self.pen.is_some() {
            return Ok(None);
        }
        properties::properties(&self.document, self.selection.ids())
    }

    /// Show a change to the selection without recording it. The first
    /// preview opens an edit on the selection as it is; each later one
    /// replaces the last. Returns false, doing nothing, while the pen, a
    /// drag or a path-edit drag is in progress, or with nothing selected.
    pub fn preview_property(&mut self, property: Property) -> Result<bool> {
        if self.property_edit.is_none() {
            if self.pen.is_some()
                || self.gesture.is_some()
                || self.path_edit.as_ref().is_some_and(PathEdit::is_dragging)
                || self.selection.is_empty()
            {
                return Ok(false);
            }
            let edit = PropertyEdit::begin(&self.document, self.selection.ids())?;
            self.property_edit = Some(edit);
        }
        let edit = self.property_edit.as_ref().expect("opened above");
        edit.preview(&mut self.document, property)?;
        Ok(true)
    }

    /// Record what the previews changed as one undo step. Returns whether
    /// anything changed: a value dragged back to where it started records
    /// nothing.
    pub fn commit_property(&mut self) -> Result<bool> {
        match self.property_edit.take() {
            Some(edit) => edit.commit(&mut self.document, &mut self.journal),
            None => Ok(false),
        }
    }

    /// Abandon the previews, putting back the values from before the first.
    /// Returns whether an edit was open.
    pub fn cancel_property(&mut self) -> Result<bool> {
        let Some(edit) = self.property_edit.take() else {
            return Ok(false);
        };
        edit.cancel(&mut self.document)?;
        Ok(true)
    }

    /// Change the selection as one undo step: a typed value, a stepped one,
    /// a removed fill. Returns whether anything changed.
    pub fn set_property(&mut self, property: Property) -> Result<bool> {
        self.cancel_property()?;
        if !self.preview_property(property)? {
            return Ok(false);
        }
        self.commit_property()
    }

    // ---- Layers --------------------------------------------------------------------
    //
    // What the layer panel does, and the shortcuts that do the same. Each is
    // one undo step, and first ends whatever else was going on — finishing a
    // pen path, as a click on a layer row does.

    /// Rename a node. Surrounding space is trimmed; a blank name, or the one
    /// it has, changes nothing.
    pub fn rename(&mut self, id: NodeId, name: &str) -> Result<bool> {
        self.end_interaction()?;
        let name = name.trim();
        if name.is_empty() || self.document.get(id)?.common.name == name {
            return Ok(false);
        }
        self.execute(Command::Rename {
            id,
            name: name.to_owned(),
        })?;
        Ok(true)
    }

    pub fn set_visible(&mut self, ids: &[NodeId], visible: bool) -> Result<bool> {
        self.end_interaction()?;
        let mut commands = Vec::new();
        for &id in ids {
            if self.document.get(id)?.common.visible != visible {
                commands.push(Command::SetVisible { id, visible });
            }
        }
        self.execute_all(commands)
    }

    pub fn set_locked(&mut self, ids: &[NodeId], locked: bool) -> Result<bool> {
        self.end_interaction()?;
        let mut commands = Vec::new();
        for &id in ids {
            if self.document.get(id)?.common.locked != locked {
                commands.push(Command::SetLocked { id, locked });
            }
        }
        self.execute_all(commands)
    }

    /// Hide the selection — or show it, when all of it is hidden already.
    pub fn toggle_visible(&mut self) -> Result<bool> {
        let ids = self.selection.ids().to_vec();
        let mut all_hidden = true;
        for &id in &ids {
            all_hidden &= !self.document.get(id)?.common.visible;
        }
        self.set_visible(&ids, all_hidden)
    }

    /// Lock the selection — or unlock it, when all of it is locked already.
    pub fn toggle_locked(&mut self) -> Result<bool> {
        let ids = self.selection.ids().to_vec();
        let mut all_locked = true;
        for &id in &ids {
            all_locked &= self.document.get(id)?.common.locked;
        }
        self.set_locked(&ids, !all_locked)
    }

    /// Move the selected layers to where they were dragged in the panel.
    pub fn move_selection(&mut self, drop: Drop) -> Result<bool> {
        self.end_interaction()?;
        let command = layers::move_command(&self.document, self.selection.ids(), drop)?;
        self.execute_some(command)
    }

    /// Bring forward or send backward, to the front or to the back.
    pub fn arrange(&mut self, arrange: Arrange) -> Result<bool> {
        self.end_interaction()?;
        let command = layers::arrange_command(&self.document, self.selection.ids(), arrange)?;
        self.execute_some(command)
    }

    /// Line the selection up by an edge or centre: with each other, or one
    /// node with the artboard. One undo step; false if nothing moved.
    pub fn align_selection(&mut self, how: Align) -> Result<bool> {
        self.end_interaction()?;
        let command = align::align_command(&self.document, self.selection.ids(), how)?;
        self.execute_some(command)
    }

    /// Space three or more selected nodes evenly. One undo step; false if
    /// nothing moved.
    pub fn distribute_selection(&mut self, axis: Distribute) -> Result<bool> {
        self.end_interaction()?;
        let command = align::distribute_command(&self.document, self.selection.ids(), axis)?;
        self.execute_some(command)
    }

    // ---- Paint as a whole ----------------------------------------------------------

    /// A press with the eyedropper: the selection takes the fill and stroke
    /// of the layer under `point` — the shape itself, inside a group too.
    /// One undo step; false if nothing was there, or nothing changed.
    pub fn eyedrop(&mut self, point: Point, tolerance: f64) -> Result<bool> {
        self.cancel_property()?;
        let Some(source) = hit::hit_test_deep(&self.document, point, tolerance)? else {
            return Ok(false);
        };
        let Some(appearance) = Appearance::of(&self.document, source)? else {
            return Ok(false);
        };
        let command = appearance::apply_command(&self.document, self.selection.ids(), &appearance)?;
        self.execute_some(command)
    }

    /// Paint the selection with the defaults (see `appearance`).
    pub fn default_paint(&mut self) -> Result<bool> {
        self.end_interaction()?;
        let command = appearance::default_command(&self.document, self.selection.ids())?;
        self.execute_some(command)
    }

    /// Swap the selection's fill and stroke colours (see `appearance`).
    pub fn swap_paint(&mut self) -> Result<bool> {
        self.end_interaction()?;
        let command = appearance::swap_command(&self.document, self.selection.ids())?;
        self.execute_some(command)
    }

    /// Put the selection in a new group, which becomes the selection.
    pub fn group_selection(&mut self) -> Result<bool> {
        self.end_interaction()?;
        if layers::roots(&self.document, self.selection.ids())?.is_empty() {
            return Ok(false);
        }
        // The group enters the arena unattached, so the batch can refer to
        // it; undo leaves it there, detached, as it does any removed node.
        let group = self.document.insert_detached(Node::group("Group"));
        let Some(command) = layers::group_command(&self.document, self.selection.ids(), group)?
        else {
            return Ok(false);
        };
        self.execute(command)?;
        self.selection.set([group]);
        Ok(true)
    }

    /// Dissolve the selected groups. What they held joins the selection in
    /// their place.
    pub fn ungroup_selection(&mut self) -> Result<bool> {
        self.end_interaction()?;
        let Some((command, freed)) = layers::ungroup_command(&self.document, self.selection.ids())?
        else {
            return Ok(false);
        };
        self.execute(command)?;
        let mut ids: Vec<NodeId> = self
            .selection
            .ids()
            .iter()
            .copied()
            .filter(|&id| self.document.is_attached(id))
            .collect();
        ids.extend(freed);
        self.selection.set(ids);
        self.hover = None;
        Ok(true)
    }

    // ---- Clipboard -----------------------------------------------------------------
    //
    // Text in and out; the shell moves it through the system clipboard.

    /// The selection as clipboard text; `None` with nothing selected.
    pub fn copy_selection(&self) -> Result<Option<String>> {
        clipboard::copy(&self.document, self.selection.ids())
    }

    /// Copy the selection, then delete it as one undo step.
    pub fn cut_selection(&mut self) -> Result<Option<String>> {
        self.end_interaction()?;
        let text = self.copy_selection()?;
        if text.is_some() {
            self.delete_selection()?;
        }
        Ok(text)
    }

    /// Paste clipboard text on top of the document, where it was copied
    /// from; what was pasted becomes the selection. False for text that is
    /// not graphicgene nodes.
    pub fn paste(&mut self, text: &str) -> Result<bool> {
        self.end_interaction()?;
        let root = self.document.root();
        let Some((command, pasted)) = clipboard::paste(&mut self.document, text, root)? else {
            return Ok(false);
        };
        self.execute(command)?;
        self.selection.set(pasted);
        Ok(true)
    }

    /// Copy the selection in place, each copy right above its original; the
    /// copies become the selection.
    pub fn duplicate_selection(&mut self) -> Result<bool> {
        self.end_interaction()?;
        let Some((command, copies)) =
            clipboard::duplicate(&mut self.document, self.selection.ids())?
        else {
            return Ok(false);
        };
        self.execute(command)?;
        self.selection.set(copies);
        Ok(true)
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
        if let Some(view) = self.text_view()? {
            return Ok(Overlay {
                mode: Some(Mode::Text),
                text: Some(view),
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
            locked: self.selection_locked()?,
            gesture: self.gesture.as_ref().map(Gesture::label),
            guides: self.gesture.as_ref().map_or([None; 2], Gesture::guides),
            pen: None,
            path: None,
            text: None,
        })
    }

    /// Whether moving and drawing snap to other layers and the artboard.
    pub fn snapping(&self) -> bool {
        !self.snapping_off
    }

    pub fn set_snapping(&mut self, on: bool) {
        self.snapping_off = !on;
    }

    // ---- Internals -----------------------------------------------------------------

    /// End whatever is in progress before a panel or menu action: abandon a
    /// drag or a property edit, finish the pen path, stop editing points.
    fn end_interaction(&mut self) -> Result<()> {
        self.cancel_gesture()?;
        self.cancel_property()?;
        self.finish_mode()?;
        Ok(())
    }

    /// Apply commands as one undo step. Returns whether there were any.
    fn execute_all(&mut self, commands: Vec<Command>) -> Result<bool> {
        self.execute_some((!commands.is_empty()).then_some(Command::Batch(commands)))
    }

    fn execute_some(&mut self, command: Option<Command>) -> Result<bool> {
        match command {
            Some(command) => {
                self.execute(command)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Whether anything selected is locked, itself or through a group.
    fn selection_locked(&self) -> Result<bool> {
        for &id in self.selection.ids() {
            let mut cursor = Some(id);
            while let Some(current) = cursor {
                let node = self.document.get(current)?;
                if node.common.locked {
                    return Ok(true);
                }
                cursor = node.common.parent;
            }
        }
        Ok(false)
    }

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
