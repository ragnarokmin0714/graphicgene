//! The editing session: the rules that tie the document, history, selection
//! and interaction modes together. Before these moved into core they could
//! only be exercised through the wasm build.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::geom::{Point, Rect, Shape, Vec2};
use graphicgene_core::gesture::{Modifiers, TransformKind};
use graphicgene_core::node::{Node, NodeId, Stroke};
use graphicgene_core::path_edit::PressOutcome;
use graphicgene_core::session::{Mode, SelectOutcome, Session};

const TOL: f64 = 4.0;
const STROKE: Stroke = Stroke {
    color: LinearRgba::BLACK,
    width: 2.0,
};
const NONE: Modifiers = Modifiers {
    shift: false,
    alt: false,
};

fn square(session: &mut Session, x: f64, y: f64) -> NodeId {
    let path = Rect::new(x, y, x + 20.0, y + 20.0).to_path(0.1);
    session
        .insert(Node::vector("Square", path, Some(LinearRgba::BLACK)))
        .unwrap()
}

fn pen_path(session: &mut Session, points: &[(f64, f64)]) {
    for &p in points {
        session.pen_press(p.into(), false, TOL, STROKE).unwrap();
        assert_eq!(session.pen_release().unwrap(), None);
    }
}

fn top_level(session: &Session) -> usize {
    let doc = session.document();
    doc.children_of(doc.root()).unwrap().len()
}

#[test]
fn undo_while_drawing_removes_pen_anchors_then_the_path() {
    let mut session = Session::new();
    pen_path(&mut session, &[(0.0, 0.0), (50.0, 0.0)]);
    assert_eq!(session.mode(), Some(Mode::Pen));
    // The unfinished path is not in the journal, yet undo is available.
    assert!(session.can_undo());
    assert!(!session.can_redo());

    assert!(session.undo().unwrap());
    assert_eq!(session.mode(), Some(Mode::Pen), "one anchor left");
    assert!(session.undo().unwrap());
    assert_eq!(
        session.mode(),
        None,
        "the last anchor took the path with it"
    );
    assert_eq!(top_level(&session), 0);
    assert!(!session.can_undo(), "nothing reached the journal");
}

#[test]
fn redo_is_blocked_while_drawing() {
    let mut session = Session::new();
    square(&mut session, 0.0, 0.0);
    session.undo().unwrap();
    pen_path(&mut session, &[(0.0, 0.0)]);
    assert!(!session.can_redo());
    assert!(!session.redo().unwrap());
}

#[test]
fn delete_means_whatever_the_mode_has_selected() {
    let mut session = Session::new();

    // Drawing: the last anchor.
    pen_path(&mut session, &[(0.0, 0.0), (50.0, 0.0), (50.0, 50.0)]);
    assert!(session.delete_selection().unwrap());
    let path = session.pen_finish().unwrap().unwrap();
    session.select_layer(path, false).unwrap();
    assert!(session.begin_path_edit().unwrap());
    assert_eq!(session.overlay().unwrap().path.unwrap().anchors.len(), 2);

    // Editing a path: the selected anchors. Emptying it deletes the node,
    // in one undo step.
    for p in [(0.0, 0.0), (50.0, 0.0)] {
        session.path_press(p.into(), TOL, true).unwrap();
        session.path_release().unwrap();
    }
    assert!(session.delete_selection().unwrap());
    assert_eq!(session.mode(), None);
    assert_eq!(top_level(&session), 0);
    session.undo().unwrap();
    assert_eq!(top_level(&session), 1);

    // Otherwise: the selected nodes.
    session.select_all().unwrap();
    assert!(session.delete_selection().unwrap());
    assert_eq!(top_level(&session), 0);
    assert!(!session.delete_selection().unwrap(), "nothing selected");
}

#[test]
fn finish_mode_leaves_pen_and_path_editing() {
    let mut session = Session::new();
    assert!(!session.finish_mode().unwrap());

    pen_path(&mut session, &[(0.0, 0.0), (40.0, 0.0)]);
    assert!(session.finish_mode().unwrap());
    assert_eq!(session.mode(), None);
    assert_eq!(
        session.selection().len(),
        1,
        "the finished path is selected"
    );

    assert!(session.begin_path_edit().unwrap());
    assert_eq!(session.mode(), Some(Mode::PathEdit));
    assert!(session.finish_mode().unwrap());
    assert_eq!(session.mode(), None);
}

#[test]
fn path_editing_needs_exactly_one_vector_selected() {
    let mut session = Session::new();
    let a = square(&mut session, 0.0, 0.0);
    square(&mut session, 40.0, 0.0);
    assert!(!session.begin_path_edit().unwrap(), "nothing selected");
    session.select_all().unwrap();
    assert!(!session.begin_path_edit().unwrap(), "two selected");
    session.select_layer(a, false).unwrap();
    assert!(session.begin_path_edit().unwrap());
}

#[test]
fn selecting_a_layer_ends_the_current_mode() {
    let mut session = Session::new();
    let a = square(&mut session, 0.0, 0.0);
    pen_path(&mut session, &[(100.0, 100.0), (150.0, 100.0)]);
    session.select_layer(a, false).unwrap();
    assert_eq!(session.mode(), None);
    assert_eq!(
        top_level(&session),
        2,
        "the pen path was finished, not lost"
    );
}

#[test]
fn undo_prunes_the_selection_and_ends_editing_a_removed_path() {
    let mut session = Session::new();
    let a = square(&mut session, 0.0, 0.0);
    session.select_layer(a, false).unwrap();
    assert!(session.begin_path_edit().unwrap());

    session.undo().unwrap(); // undoes the insert
    assert!(session.selection().is_empty());
    assert_eq!(session.mode(), None);
}

#[test]
fn undo_keeps_path_editing_and_forgets_anchors_that_no_longer_exist() {
    let mut session = Session::new();
    let a = square(&mut session, 0.0, 0.0);
    session.select_layer(a, false).unwrap();
    session.begin_path_edit().unwrap();
    // Insert an anchor on the top edge; it becomes the selected one.
    assert_eq!(
        session
            .path_press(Point::new(10.0, 0.0), TOL, false)
            .unwrap(),
        PressOutcome::Segment
    );
    session.path_release().unwrap();
    session.undo().unwrap();

    assert_eq!(session.mode(), Some(Mode::PathEdit));
    let path = session.overlay().unwrap().path.unwrap();
    assert_eq!(path.anchors.len(), 4);
    assert!(path.anchors.iter().all(|&(_, selected)| !selected));
}

#[test]
fn clicks_select_drag_and_marquee() {
    let mut session = Session::new();
    let a = square(&mut session, 0.0, 0.0);
    assert_eq!(
        session
            .select_at(Point::new(10.0, 10.0), false, TOL)
            .unwrap(),
        SelectOutcome::Drag
    );
    assert!(
        session
            .begin_transform(TransformKind::Move, Point::new(10.0, 10.0))
            .unwrap()
    );
    session
        .update_gesture(Point::new(30.0, 10.0), NONE)
        .unwrap();
    assert!(session.busy());
    session.end_gesture().unwrap();
    assert!(!session.busy());
    let moved = session.document().world_bounds(a).unwrap();
    assert!((moved.x0 - 20.0).abs() < 1e-9);

    assert_eq!(
        session
            .select_at(Point::new(30.0, 10.0), true, TOL)
            .unwrap(),
        SelectOutcome::Hit,
        "Shift on a selected node deselects without dragging"
    );
    assert_eq!(
        session
            .select_at(Point::new(200.0, 200.0), false, TOL)
            .unwrap(),
        SelectOutcome::Miss
    );
}

#[test]
fn nudges_move_nodes_or_selected_anchors() {
    let mut session = Session::new();
    let a = square(&mut session, 0.0, 0.0);
    session.select_layer(a, false).unwrap();
    assert!(session.nudge(Vec2::new(5.0, 0.0)).unwrap());
    assert!((session.document().world_bounds(a).unwrap().x0 - 5.0).abs() < 1e-9);

    session.begin_path_edit().unwrap();
    assert!(
        !session.nudge(Vec2::new(5.0, 0.0)).unwrap(),
        "no anchors selected"
    );
    session
        .path_press(Point::new(5.0, 0.0), TOL, false)
        .unwrap();
    session.path_release().unwrap();
    assert!(session.nudge(Vec2::new(0.0, -5.0)).unwrap());
    let bounds = session.document().world_bounds(a).unwrap();
    assert!(
        (bounds.y0 + 5.0).abs() < 1e-9,
        "one corner moved up: {bounds:?}"
    );
}

#[test]
fn saving_drops_detached_nodes_and_loading_resets_the_session() {
    let mut session = Session::new();
    for _ in 0..3 {
        square(&mut session, 0.0, 0.0);
        session.undo().unwrap();
    }
    let kept = square(&mut session, 0.0, 0.0);
    let text = session.save().unwrap();

    let mut other = Session::new();
    pen_path(&mut other, &[(0.0, 0.0), (10.0, 0.0)]);
    other.load(&text).unwrap();
    assert_eq!(other.mode(), None, "loading ends the pen");
    assert!(!other.can_undo(), "and forgets the history");
    assert!(
        other.document().is_attached(kept),
        "ids survive the round trip"
    );
    assert_eq!(other.document().len(), 2, "root and the one live node");

    assert!(other.load("{ not json").is_err());
    assert!(
        other.document().is_attached(kept),
        "a failed load changes nothing"
    );
}

#[test]
fn layer_rows_come_topmost_first_with_selection() {
    let mut session = Session::new();
    let below = square(&mut session, 0.0, 0.0);
    let above = square(&mut session, 10.0, 0.0);
    session.select_layer(below, false).unwrap();
    let rows = session.layer_rows().unwrap();
    assert_eq!(
        rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        [above, below]
    );
    assert_eq!(
        rows.iter().map(|r| r.selected).collect::<Vec<_>>(),
        [false, true]
    );
}

#[test]
fn the_layers_version_holds_still_through_a_drag_preview() {
    let mut session = Session::new();
    let a = square(&mut session, 0.0, 0.0);
    let v0 = session.layers_version();

    session.select_layer(a, false).unwrap();
    let v1 = session.layers_version();
    assert_ne!(v1, v0, "selection shows in the rows");

    session
        .begin_transform(TransformKind::Move, Point::new(5.0, 5.0))
        .unwrap();
    for step in 1..10 {
        let p = Point::new(5.0 + step as f64, 5.0);
        session.update_gesture(p, NONE).unwrap();
        assert_eq!(session.layers_version(), v1, "a preview never changes rows");
    }
    session.end_gesture().unwrap();
    let v2 = session.layers_version();
    assert_ne!(v2, v1, "the commit may have");

    let text = session.save().unwrap();
    session.load(&text).unwrap();
    assert_ne!(session.layers_version(), v2, "a load always counts");
}

#[test]
fn prepare_render_reports_document_changes_only() {
    let mut session = Session::new();
    let a = square(&mut session, 0.0, 0.0);
    assert!(
        session.prepare_render().unwrap().everything,
        "a new document draws in full"
    );
    assert!(session.prepare_render().unwrap().is_empty());

    // Selection and hover are not document changes.
    session.select_layer(a, false).unwrap();
    session.hover(Point::new(5.0, 5.0), TOL).unwrap();
    assert!(session.prepare_render().unwrap().is_empty());

    session
        .begin_transform(TransformKind::Move, Point::new(5.0, 5.0))
        .unwrap();
    session.update_gesture(Point::new(8.0, 5.0), NONE).unwrap();
    let changes = session.prepare_render().unwrap();
    assert!(!changes.structure);
    assert!(changes.nodes.contains(&a), "the dragged node is reported");

    session.cancel_gesture().unwrap();
    session.delete_selection().unwrap();
    assert!(session.prepare_render().unwrap().structure);
}

#[test]
fn the_overlay_follows_the_mode() {
    let mut session = Session::new();
    let a = square(&mut session, 0.0, 0.0);
    assert!(session.overlay().unwrap().frame.is_none());

    session.select_layer(a, false).unwrap();
    let overlay = session.overlay().unwrap();
    assert!(overlay.frame.is_some() && overlay.mode.is_none());

    session.begin_path_edit().unwrap();
    let overlay = session.overlay().unwrap();
    assert_eq!(overlay.mode, Some(Mode::PathEdit));
    assert!(overlay.frame.is_none(), "points replace the frame");
    assert!(overlay.path.is_some());

    session.finish_mode().unwrap();
    pen_path(&mut session, &[(100.0, 100.0)]);
    session.pen_hover(Point::new(150.0, 120.0), TOL);
    let overlay = session.overlay().unwrap();
    assert_eq!(overlay.mode, Some(Mode::Pen));
    assert!(overlay.pen.unwrap().preview.is_some());
}
