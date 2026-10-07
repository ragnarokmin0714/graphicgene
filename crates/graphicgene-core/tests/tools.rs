//! Canvas input through the session: presses, moves and releases routed by
//! the tool in hand, and the keys that back out of or finish them. What the
//! web page used to decide, a desktop shell now gets from core.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::geom::{Point, Rect, Shape};
use graphicgene_core::gesture::{Frame, Modifiers};
use graphicgene_core::node::{Node, NodeId};
use graphicgene_core::session::{Grab, Mode, Pointer, Session, Tool};

const RED: LinearRgba = LinearRgba::new(1.0, 0.0, 0.0, 1.0);

fn at(x: f64, y: f64) -> Pointer {
    Pointer {
        point: Point::new(x, y),
        modifiers: Modifiers::default(),
        hit_tolerance: 4.0,
        pick_tolerance: 6.0,
    }
}

fn shifted(mut pointer: Pointer) -> Pointer {
    pointer.modifiers.shift = true;
    pointer
}

fn drag(session: &mut Session, from: Pointer, to: Pointer, grab: Option<Grab>) {
    session.pointer_down(from, grab, RED).unwrap();
    let mid = at(
        (from.point.x + to.point.x) / 2.0,
        (from.point.y + to.point.y) / 2.0,
    );
    session
        .pointer_move(Pointer {
            modifiers: to.modifiers,
            ..mid
        })
        .unwrap();
    session.pointer_move(to).unwrap();
    session.pointer_up().unwrap();
}

fn click(session: &mut Session, pointer: Pointer) {
    session.pointer_down(pointer, None, RED).unwrap();
    session.pointer_up().unwrap();
}

fn square(session: &mut Session) -> NodeId {
    let path = Rect::new(0.0, 0.0, 20.0, 20.0).to_path(0.1);
    session
        .insert(Node::vector("Square", path, Some(RED)))
        .unwrap()
}

/// The selection frame's top-left corner and size.
fn frame(session: &Session) -> (Point, f64, f64) {
    let frame = Frame::of(session.document(), session.selection().ids())
        .unwrap()
        .expect("something is selected");
    let (w, h) = frame.size();
    (frame.corners()[0], w, h)
}

fn count(session: &Session) -> usize {
    let doc = session.document();
    doc.children_of(doc.root()).unwrap().len()
}

#[test]
fn the_select_tool_routes_a_press_by_what_is_under_it() {
    let mut session = Session::new();
    let a = square(&mut session);

    // On a shape: select it and move it, as one step.
    drag(&mut session, at(10.0, 10.0), at(40.0, 25.0), None);
    assert_eq!(session.selection().ids(), [a]);
    assert_eq!(frame(&session).0, Point::new(30.0, 15.0));
    session.undo().unwrap();
    assert_eq!(frame(&session).0, Point::new(0.0, 0.0));

    // On empty space: a marquee, which picks up what it touches.
    session.clear_selection().unwrap();
    drag(&mut session, at(100.0, 100.0), at(15.0, 15.0), None);
    assert_eq!(session.selection().ids(), [a]);

    // On a handle the shell found: scale from it, or rotate.
    drag(
        &mut session,
        at(20.0, 20.0),
        at(40.0, 30.0),
        Some(Grab::Scale { u: 1.0, v: 1.0 }),
    );
    let (corner, w, h) = frame(&session);
    assert_eq!(
        (corner, w.round(), h.round()),
        (Point::new(0.0, 0.0), 40.0, 30.0)
    );
    drag(
        &mut session,
        at(45.0, -5.0),
        at(50.0, 20.0),
        Some(Grab::Rotate),
    );
    assert_ne!(
        frame(&session).0,
        Point::new(0.0, 0.0),
        "turned about its centre"
    );

    // Shift on a selected shape takes it out, and starts nothing.
    drag(
        &mut session,
        shifted(at(20.0, 15.0)),
        shifted(at(90.0, 90.0)),
        None,
    );
    assert!(session.selection().is_empty());
}

#[test]
fn shape_tools_draw_then_hand_back_to_select() {
    let mut session = Session::new();
    session.set_tool(Tool::Rect).unwrap();
    drag(&mut session, at(10.0, 10.0), at(50.0, 40.0), None);
    assert_eq!(session.tool(), Tool::Select);
    let (corner, w, h) = frame(&session);
    assert_eq!((corner, w, h), (Point::new(10.0, 10.0), 40.0, 30.0));

    session.set_tool(Tool::Ellipse).unwrap();
    click(&mut session, at(200.0, 200.0));
    assert_eq!(
        (frame(&session).1, frame(&session).2),
        (100.0, 100.0),
        "a click places a default size"
    );
    assert_eq!((session.tool(), count(&session)), (Tool::Select, 2));
}

#[test]
fn the_pen_draws_until_its_path_closes() {
    let mut session = Session::new();
    session.set_tool(Tool::Pen).unwrap();
    for (x, y) in [(0.0, 0.0), (50.0, 0.0), (50.0, 50.0)] {
        click(&mut session, at(x, y));
    }
    assert_eq!(session.mode(), Some(Mode::Pen));
    assert!(
        session.pointer_move(at(20.0, 30.0)).unwrap(),
        "the preview follows the pointer"
    );

    click(&mut session, at(1.0, 1.0));
    assert_eq!(
        (session.mode(), session.tool()),
        (None, Tool::Select),
        "closed"
    );
    assert_eq!(count(&session), 1);
    // The double-click that closed it does not start editing its points.
    session.double_click(at(1.0, 1.0)).unwrap();
    assert_eq!(session.mode(), None);
    session.undo().unwrap();
    assert_eq!(count(&session), 0, "one step");
}

#[test]
fn a_double_click_edits_points() {
    let mut session = Session::new();
    square(&mut session);
    click(&mut session, at(10.0, 10.0));
    session.double_click(at(10.0, 10.0)).unwrap();
    assert_eq!(session.mode(), Some(Mode::PathEdit));

    // Presses go to the points now: drag a corner.
    drag(&mut session, at(20.0, 20.0), at(30.0, 40.0), None);
    assert_eq!(frame(&session).1.round(), 30.0);
    session.undo().unwrap();
    assert_eq!(session.mode(), Some(Mode::PathEdit), "undo keeps editing");
    session.double_click(at(200.0, 200.0)).unwrap();
    assert_eq!(session.mode(), None, "empty space stops it");
}

#[test]
fn escape_backs_out_one_level_and_enter_finishes() {
    let mut session = Session::new();
    square(&mut session);
    click(&mut session, at(10.0, 10.0));

    // The drag first: put back as it was.
    session.pointer_down(at(10.0, 10.0), None, RED).unwrap();
    session.pointer_move(at(60.0, 60.0)).unwrap();
    session.escape().unwrap();
    session.pointer_up().unwrap();
    assert_eq!(frame(&session).0, Point::new(0.0, 0.0));
    assert!(!session.can_redo() && session.selection().len() == 1);

    // Then the tool, then the selection.
    session.set_tool(Tool::Rect).unwrap();
    session.escape().unwrap();
    assert_eq!(
        (session.tool(), session.selection().len()),
        (Tool::Select, 1)
    );
    session.escape().unwrap();
    assert!(session.selection().is_empty());

    // A pen path is kept, and the pen put down.
    session.set_tool(Tool::Pen).unwrap();
    click(&mut session, at(100.0, 0.0));
    click(&mut session, at(150.0, 0.0));
    session.escape().unwrap();
    assert_eq!(
        (session.mode(), session.tool(), count(&session)),
        (None, Tool::Select, 2)
    );

    // Enter edits the selected path's points, and finishes that too.
    session.enter().unwrap();
    assert_eq!(session.mode(), Some(Mode::PathEdit));
    session.enter().unwrap();
    assert_eq!(session.mode(), None);
}

#[test]
fn a_press_away_from_the_path_stops_editing_points() {
    let mut session = Session::new();
    let first = square(&mut session);
    let other = session
        .insert(Node::vector(
            "Other",
            Rect::new(100.0, 0.0, 120.0, 20.0).to_path(0.1),
            Some(RED),
        ))
        .unwrap();
    click(&mut session, at(10.0, 10.0));
    session.enter().unwrap();
    assert_eq!(session.mode(), Some(Mode::PathEdit));

    // Inside the shape, or with Shift held anywhere, a press stays with the
    // points.
    click(&mut session, at(10.0, 10.0));
    click(&mut session, shifted(at(60.0, 60.0)));
    assert_eq!(session.mode(), Some(Mode::PathEdit));

    // On another shape it stops editing, and selects that one.
    click(&mut session, at(110.0, 10.0));
    assert_eq!(session.mode(), None);
    assert_eq!(session.selection().ids(), &[other]);

    // On empty space it stops editing, and clears the selection.
    click(&mut session, at(10.0, 10.0));
    session.enter().unwrap();
    click(&mut session, at(60.0, 60.0));
    assert_eq!(session.mode(), None);
    assert!(session.selection().is_empty());

    // Escape stops it too, keeping the path selected.
    click(&mut session, at(10.0, 10.0));
    session.enter().unwrap();
    session.escape().unwrap();
    assert_eq!(session.mode(), None);
    assert_eq!(session.selection().ids(), &[first]);
}

#[test]
fn picking_up_a_tool_finishes_the_pen_path() {
    let mut session = Session::new();
    session.set_tool(Tool::Pen).unwrap();
    click(&mut session, at(0.0, 0.0));
    click(&mut session, at(40.0, 0.0));
    session.set_tool(Tool::Rect).unwrap();
    assert_eq!((session.mode(), count(&session)), (None, 1));
}

#[test]
fn a_lost_pointer_abandons_the_drag() {
    let mut session = Session::new();
    square(&mut session);
    session.pointer_down(at(10.0, 10.0), None, RED).unwrap();
    session.pointer_move(at(70.0, 70.0)).unwrap();
    session.pointer_cancel().unwrap();
    session.pointer_up().unwrap();
    assert_eq!(frame(&session).0, Point::new(0.0, 0.0));
    session.undo().unwrap();
    assert_eq!(count(&session), 0, "only the insert was recorded");
}

#[test]
fn hovering_reports_only_changes() {
    let mut session = Session::new();
    square(&mut session);
    assert!(
        session.pointer_move(at(10.0, 10.0)).unwrap(),
        "onto the square"
    );
    assert!(
        !session.pointer_move(at(12.0, 10.0)).unwrap(),
        "still on it"
    );
    assert!(session.pointer_move(at(90.0, 90.0)).unwrap(), "off again");
    session.set_tool(Tool::Rect).unwrap();
    assert!(
        !session.pointer_move(at(10.0, 10.0)).unwrap(),
        "no hover while drawing"
    );
}

#[test]
fn direct_selection_reaches_into_groups_and_edits_points() {
    let mut session = Session::new();
    let first = square(&mut session);
    let second = session
        .insert(Node::vector(
            "Other",
            Rect::new(100.0, 0.0, 120.0, 20.0).to_path(0.1),
            Some(RED),
        ))
        .unwrap();
    session.select_all().unwrap();
    session.group_selection().unwrap();
    session.clear_selection().unwrap();

    session.set_tool(Tool::Direct).unwrap();
    session.pointer_move(at(110.0, 10.0)).unwrap();
    let hover = session.overlay().unwrap().hover.expect("hovering a shape");
    assert_eq!(hover.bounding_box().x0, 100.0, "the shape, not its group");
    click(&mut session, at(110.0, 10.0));
    assert_eq!(session.selection().ids(), &[second], "the shape itself");
    assert_eq!(session.mode(), Some(Mode::PathEdit));

    // A press on another shape's corner drags that point at once.
    drag(&mut session, at(0.0, 0.0), at(-10.0, -5.0), None);
    assert_eq!(session.selection().ids(), &[first]);
    let path = session.document().vector_path(first).unwrap().clone();
    let b = path.bounding_box();
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (-10.0, -5.0, 20.0, 20.0));
    assert_eq!(session.tool(), Tool::Direct, "the tool stays in hand");
    session.undo().unwrap();
    let b = session
        .document()
        .vector_path(first)
        .unwrap()
        .bounding_box();
    assert_eq!((b.x0, b.y0), (0.0, 0.0), "one undo step");
}

#[test]
fn direct_selection_drags_a_shape_by_its_edge_without_adding_a_point() {
    let mut session = Session::new();
    let id = square(&mut session);
    let elements = session.document().vector_path(id).unwrap().elements().len();
    session.set_tool(Tool::Direct).unwrap();
    // The middle of the top edge, not yet being edited: the whole shape.
    drag(&mut session, at(10.0, 0.0), at(40.0, 40.0), None);
    assert_eq!(frame(&session).0, Point::new(30.0, 40.0));
    let path = session.document().vector_path(id).unwrap();
    assert_eq!(path.elements().len(), elements, "no point inserted");
    assert_eq!(session.mode(), Some(Mode::PathEdit));

    // Inside it, while editing, the whole shape too.
    drag(&mut session, at(40.0, 50.0), at(60.0, 50.0), None);
    assert_eq!(frame(&session).0, Point::new(50.0, 40.0));

    // On nothing: editing stops, and a marquee picks the shape back up.
    drag(&mut session, at(200.0, 200.0), at(55.0, 45.0), None);
    assert_eq!(session.mode(), None);
    assert_eq!(session.selection().ids(), &[id]);
}

#[test]
fn the_eyedropper_paints_the_selection_like_what_it_clicks() {
    use graphicgene_core::node::{Dash, NodeKind, Stroke};
    use graphicgene_core::paint::Paint;
    const BLUE: LinearRgba = LinearRgba::new(0.0, 0.0, 1.0, 1.0);

    let mut session = Session::new();
    let target = square(&mut session);
    let mut source = Node::vector(
        "Source",
        Rect::new(100.0, 0.0, 120.0, 20.0).to_path(0.1),
        Some(BLUE),
    );
    let stroke = Stroke {
        dash: Some(Dash {
            length: 4.0,
            gap: 2.0,
        }),
        ..Stroke::solid(RED, 3.0)
    };
    if let NodeKind::Vector(v) = &mut source.kind {
        v.stroke = Some(stroke);
    }
    session.insert(source).unwrap();
    click(&mut session, at(10.0, 10.0));
    session.set_tool(Tool::Eyedropper).unwrap();

    let paint = |session: &Session| match &session.document().get(target).unwrap().kind {
        NodeKind::Vector(v) => (v.fill.clone(), v.stroke),
        _ => unreachable!(),
    };
    let before = paint(&session);
    click(&mut session, at(60.0, 60.0));
    assert_eq!(paint(&session), before, "nothing there, nothing taken");

    click(&mut session, at(110.0, 10.0));
    assert_eq!(paint(&session), (Some(Paint::Solid(BLUE)), Some(stroke)));
    assert_eq!(session.selection().ids(), &[target], "the selection stays");
    assert_eq!(session.tool(), Tool::Eyedropper, "and so does the tool");
    session.undo().unwrap();
    assert_eq!(paint(&session), before, "one undo step");
}

#[test]
fn a_press_with_the_hand_tool_is_the_shells_alone() {
    let mut session = Session::new();
    let id = square(&mut session);
    session.set_tool(Tool::Hand).unwrap();
    drag(&mut session, at(10.0, 10.0), at(50.0, 50.0), None);
    assert!(session.selection().is_empty(), "nothing selected");
    let b = session.document().world_bounds(id).unwrap();
    assert_eq!((b.x0, b.y0), (0.0, 0.0), "nothing moved");
}
