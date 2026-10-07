//! Canvas input: what a press does with the tool in hand, and the keys that
//! back out of or finish what it started.
//!
//! A shell reports presses, moves and releases in document space, with the
//! selection handle its own hit-test found under a press — handles are
//! screen-sized, so finding them is the view's job — and the session routes
//! them: the pen, point editing, a drag on the selection, a new shape, a
//! marquee. The shell keeps only what is its own: panning the view — with
//! the hand tool too, whose presses never reach here — and the cursor.

use crate::color::LinearRgba;
use crate::error::Result;
use crate::geom::Point;
use crate::gesture::{Modifiers, ShapeKind, TransformKind};
use crate::hit;
use crate::node::{NodeId, NodeKind, Stroke};
use crate::path_edit::{PathEdit, PressOutcome};

use super::{Mode, SelectOutcome, Session};

/// Stroke width for paths drawn with the pen, in document units.
pub const PEN_STROKE_WIDTH: f64 = 2.0;

/// What the pointer does on the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    /// Illustrator's direct selection: straight to a shape's points, and to
    /// shapes inside groups.
    Direct,
    Rect,
    Ellipse,
    Pen,
    Text,
    /// Gives the selection the fill and stroke of the layer clicked.
    Eyedropper,
    /// Drags the view; the shell pans, so the session ignores its presses.
    Hand,
}

/// The selection handle under a press, as the shell's hit-test found it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Grab {
    /// The handle at unit coordinates `(u, v)` of the frame.
    Scale { u: f64, v: f64 },
    /// Just outside a corner.
    Rotate,
}

/// Where the pointer is, with the keys held and how near counts as on
/// something — screen distances divided by the zoom, which only the shell
/// knows.
#[derive(Debug, Clone, Copy)]
pub struct Pointer {
    pub point: Point,
    pub modifiers: Modifiers,
    /// How far outside a shape still hits it.
    pub hit_tolerance: f64,
    /// How near a point or handle counts as pressing it.
    pub pick_tolerance: f64,
}

/// Which interaction a press started; its moves and release go there too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    Pen,
    Path,
    Gesture,
    /// A press that did all it does — started typing, or took colours:
    /// nothing to drag.
    Tap,
}

impl Session {
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// Pick up another tool. Whatever was going on ends first: a pen path
    /// is finished, point editing stopped.
    pub fn set_tool(&mut self, tool: Tool) -> Result<()> {
        self.end_interaction()?;
        self.tool = tool;
        Ok(())
    }

    /// A press on the canvas. `color` is for whatever it starts drawing: a
    /// new shape's fill, or a pen path's stroke.
    pub fn pointer_down(
        &mut self,
        at: Pointer,
        grab: Option<Grab>,
        color: LinearRgba,
    ) -> Result<()> {
        self.pointer_cancel()?;
        self.snap_tolerance = at.pick_tolerance;
        self.snap_held_off = at.modifiers.ctrl;
        let Pointer {
            point, modifiers, ..
        } = at;
        // A press away from text being typed finishes it and puts the text
        // tool down: from here on it is a select press.
        if self.editing_text() {
            self.commit_text()?;
            self.tool = Tool::Select;
        }
        if self.tool == Tool::Hand {
            return Ok(());
        }
        let route = if self.tool == Tool::Text {
            match hit::hit_test_deep(&self.document, point, at.hit_tolerance)? {
                Some(id) if self.is_text(id)? => {
                    self.edit_text(id)?;
                }
                _ => {
                    self.begin_text(point, color)?;
                }
            }
            Route::Tap
        } else if self.tool == Tool::Eyedropper {
            self.eyedrop(point, at.hit_tolerance)?;
            Route::Tap
        } else if self.tool == Tool::Direct {
            self.direct_press(at)?
        } else if self.tool == Tool::Pen {
            let stroke = Stroke::solid(color, PEN_STROKE_WIDTH);
            self.pen_press(point, modifiers.shift, at.pick_tolerance, stroke)?;
            Route::Pen
        } else if self.mode() == Some(Mode::PathEdit) && self.press_keeps_path_edit(at)? {
            Route::Path
        } else {
            match self.tool {
                Tool::Rect => self.begin_create(ShapeKind::Rect, color, point)?,
                Tool::Ellipse => self.begin_create(ShapeKind::Ellipse, color, point)?,
                _ => {
                    match grab {
                        Some(Grab::Scale { u, v }) => {
                            self.begin_transform(TransformKind::Scale { u, v }, point)?;
                        }
                        Some(Grab::Rotate) => {
                            self.begin_transform(TransformKind::Rotate, point)?;
                        }
                        None => match self.select_at(point, modifiers.shift, at.hit_tolerance)? {
                            SelectOutcome::Drag => {
                                self.begin_move(point, modifiers.alt)?;
                            }
                            SelectOutcome::Miss => self.begin_marquee(point, modifiers.shift)?,
                            SelectOutcome::Hit => {}
                        },
                    }
                    self.clear_hover();
                }
            }
            Route::Gesture
        };
        self.route = Some(route);
        self.last_route = Some(route);
        Ok(())
    }

    /// The pointer moved. Pressed, it drives what the press started; not,
    /// it tracks what is under it. Returns whether that changed anything
    /// drawn over the artwork — always, while pressed.
    pub fn pointer_move(&mut self, at: Pointer) -> Result<bool> {
        self.snap_tolerance = at.pick_tolerance;
        self.snap_held_off = at.modifiers.ctrl;
        let Pointer {
            point, modifiers, ..
        } = at;
        match self.route {
            Some(Route::Pen) => self.pen_drag(point, modifiers.shift)?,
            Some(Route::Path) => self.path_drag(point, modifiers)?,
            Some(Route::Gesture) => self.update_gesture(point, modifiers)?,
            Some(Route::Tap) => return Ok(false),
            None if self.tool == Tool::Pen => return Ok(self.pen_hover(point, at.pick_tolerance)),
            None if matches!(self.tool, Tool::Select | Tool::Direct) && self.mode().is_none() => {
                return self.hover(point, at.hit_tolerance);
            }
            None => return Ok(false),
        }
        Ok(true)
    }

    /// The press ended. A shape drawn, or a pen path closed, hands the
    /// pointer back to the select tool.
    pub fn pointer_up(&mut self) -> Result<()> {
        let finished = match self.route.take() {
            Some(Route::Pen) => self.pen_release()?.is_some(),
            Some(Route::Path) => {
                self.path_release()?;
                false
            }
            Some(Route::Gesture) => self.end_gesture()?.is_some(),
            Some(Route::Tap) | None => false,
        };
        if finished {
            self.tool = Tool::Select;
        }
        Ok(())
    }

    /// The browser took the pointer away: abandon a drag rather than commit
    /// it half-way. A pen press keeps its anchor.
    pub fn pointer_cancel(&mut self) -> Result<()> {
        match self.route.take() {
            Some(Route::Path) => self.path_cancel_drag()?,
            Some(Route::Gesture) => {
                self.cancel_gesture()?;
            }
            Some(Route::Pen | Route::Tap) | None => {}
        }
        Ok(())
    }

    /// A double-click with either selection tool types into selected text,
    /// or edits a path's points; while editing points, it toggles one
    /// between corner and curve, or stops on empty space. The one that
    /// finishes a pen path is not one of these.
    pub fn double_click(&mut self, at: Pointer) -> Result<()> {
        if !self.selecting() || self.last_route == Some(Route::Pen) {
            return Ok(());
        }
        if self.mode() == Some(Mode::PathEdit) {
            self.path_double_click(at.point, at.pick_tolerance)?;
        } else if !self.edit_selected_text()? {
            self.begin_path_edit()?;
        }
        Ok(())
    }

    /// Escape backs out one level: the drag; then the pen path or point
    /// editing, kept rather than discarded; then the tool; then the
    /// selection.
    pub fn escape(&mut self) -> Result<()> {
        if self.cancel_gesture()? {
            return Ok(());
        }
        if self.finish_mode()? {
            if matches!(self.tool, Tool::Pen | Tool::Text) {
                self.tool = Tool::Select;
            }
            return Ok(());
        }
        if self.tool != Tool::Select {
            self.tool = Tool::Select;
            return Ok(());
        }
        self.clear_selection()
    }

    /// Enter finishes the pen path or point editing — or, with the select
    /// tool, starts typing into the selected text or editing the selected
    /// path's points.
    pub fn enter(&mut self) -> Result<()> {
        if self.finish_mode()? {
            if matches!(self.tool, Tool::Pen | Tool::Text) {
                self.tool = Tool::Select;
            }
        } else if self.selecting() && !self.edit_selected_text()? {
            self.begin_path_edit()?;
        }
        Ok(())
    }

    /// A press while editing points. On a point, a handle or the outline it
    /// edits them; anywhere else on the path it only clears the picked
    /// points. Off the path altogether it stops editing and returns false,
    /// so the press carries on as a select press — clicking another shape
    /// selects it, clicking empty space clears the selection, as clicking
    /// away does in other editors. Shift keeps editing: it adds points.
    fn press_keeps_path_edit(&mut self, at: Pointer) -> Result<bool> {
        let outcome = self.path_press(at.point, at.pick_tolerance, at.modifiers.shift)?;
        if outcome != PressOutcome::Miss || at.modifiers.shift {
            return Ok(true);
        }
        let editing = self.path_edit.as_ref().map(PathEdit::id);
        if editing.is_some()
            && hit::hit_test(&self.document, at.point, at.hit_tolerance)? == editing
        {
            return Ok(true);
        }
        self.end_path_edit()?;
        Ok(false)
    }

    /// A press with the direct selection tool, which works on points. On a
    /// point, a handle or the outline of the path being edited, it edits
    /// them, as point editing does. On any other shape — inside a group too
    /// — it selects that shape and edits its points, dragging the point
    /// pressed, or else the whole shape, rather than inserting a point where
    /// it was grabbed; on text it selects and drags the text. On nothing it
    /// stops editing and draws a marquee.
    fn direct_press(&mut self, at: Pointer) -> Result<Route> {
        let Pointer {
            point, modifiers, ..
        } = at;
        if self.mode() == Some(Mode::PathEdit) {
            let outcome = self.path_press(point, at.pick_tolerance, modifiers.shift)?;
            if outcome != PressOutcome::Miss || modifiers.shift {
                return Ok(Route::Path);
            }
        }
        let editing = self.path_edit.as_ref().map(PathEdit::id);
        let hit = hit::hit_test_deep(&self.document, point, at.hit_tolerance)?;
        match hit {
            Some(id) if Some(id) != editing => {
                self.end_path_edit()?;
                self.cancel_property()?;
                self.selection.set([id]);
                self.clear_hover();
                let picks = self.begin_path_edit()?
                    && self.path_edit.as_ref().map_or(Ok(false), |edit| {
                        edit.picks_point(&self.document, point, at.pick_tolerance)
                    })?;
                if picks {
                    self.path_press(point, at.pick_tolerance, false)?;
                    return Ok(Route::Path);
                }
                self.begin_transform(TransformKind::Move, point)?;
            }
            Some(_) => {
                self.begin_transform(TransformKind::Move, point)?;
            }
            None => {
                self.end_path_edit()?;
                self.begin_marquee(point, modifiers.shift)?;
            }
        }
        Ok(Route::Gesture)
    }

    /// The select tool, or the direct selection tool.
    fn selecting(&self) -> bool {
        matches!(self.tool, Tool::Select | Tool::Direct)
    }

    /// Type into the one selected node, if it is text.
    fn edit_selected_text(&mut self) -> Result<bool> {
        match *self.selection.ids() {
            [id] if self.is_text(id)? => self.edit_text(id),
            _ => Ok(false),
        }
    }

    fn is_text(&self, id: NodeId) -> Result<bool> {
        Ok(matches!(self.document.get(id)?.kind, NodeKind::Text(_)))
    }
}
