//! Snapping: moving and drawing catch on other layers' and the artboard's
//! edges and centre lines, within the pick distance, with a guide drawn
//! for each axis that caught.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::geom::{Point, Rect, Shape, Size};
use graphicgene_core::gesture::Modifiers;
use graphicgene_core::node::{Node, NodeId};
use graphicgene_core::session::{Pointer, Session, Tool};
use graphicgene_core::snap::Guide;

const RED: LinearRgba = LinearRgba::new(1.0, 0.0, 0.0, 1.0);

fn at(x: f64, y: f64) -> Pointer {
    with(x, y, Modifiers::default())
}

fn with(x: f64, y: f64, modifiers: Modifiers) -> Pointer {
    Pointer {
        point: Point::new(x, y),
        modifiers,
        hit_tolerance: 4.0,
        pick_tolerance: 6.0,
    }
}

fn rect(session: &mut Session, x: f64, y: f64, w: f64, h: f64) -> NodeId {
    let path = Rect::new(x, y, x + w, y + h).to_path(0.1);
    session.insert(Node::vector("r", path, Some(RED))).unwrap()
}

fn bounds(session: &Session, id: NodeId) -> Rect {
    session.document().world_bounds(id).unwrap()
}

/// A 1000 × 1000 artboard, so its centre lines (500) stay out of the way,
/// with a square at (100, 100)–(140, 140) and another to drag at
/// (300, 300)–(320, 320).
fn two_squares() -> (Session, NodeId, NodeId) {
    let mut session = Session::with_artboard(Size::new(1000.0, 1000.0));
    let fixed = rect(&mut session, 100.0, 100.0, 40.0, 40.0);
    let moving = rect(&mut session, 300.0, 300.0, 20.0, 20.0);
    (session, fixed, moving)
}

#[test]
fn a_moved_edge_catches_on_another_layers_edge_and_shows_a_guide() {
    let (mut session, _, moving) = two_squares();
    // Grab the moving square at its middle and bring its left edge to
    // x = 143, three units right of the fixed square's right edge.
    session.pointer_down(at(310.0, 310.0), None, RED).unwrap();
    session.pointer_move(at(153.0, 400.0)).unwrap();
    let overlay = session.overlay().unwrap();
    assert_eq!(bounds(&session, moving).x0, 140.0, "caught on x = 140");
    let [Some(guide), None] = overlay.guides else {
        panic!("one vertical guide: {:?}", overlay.guides);
    };
    assert_eq!(guide.from.x, 140.0);
    assert_eq!(guide.to.x, 140.0);
    // From the fixed square's top down to the moved one's bottom.
    assert_eq!((guide.from.y, guide.to.y), (100.0, 410.0));

    session.pointer_up().unwrap();
    assert_eq!(bounds(&session, moving).x0, 140.0, "and stays there");
    assert_eq!(
        session.overlay().unwrap().guides,
        [None, None],
        "no guide after"
    );
    session.undo().unwrap();
    assert_eq!(bounds(&session, moving).x0, 300.0, "one undo step");
}

#[test]
fn centres_catch_too_and_both_axes_at_once() {
    let (mut session, _, moving) = two_squares();
    // The moving square's centre to (121, 118): the fixed one's is (120, 120).
    session.pointer_down(at(310.0, 310.0), None, RED).unwrap();
    session.pointer_move(at(121.0, 118.0)).unwrap();
    let b = bounds(&session, moving);
    assert_eq!(b.center(), Point::new(120.0, 120.0));
    assert!(
        session
            .overlay()
            .unwrap()
            .guides
            .iter()
            .all(Option::is_some)
    );
}

#[test]
fn too_far_ctrl_held_or_snapping_off_and_nothing_catches() {
    let (mut session, _, moving) = two_squares();
    // Left edge to 150: ten past the fixed square's edge, beyond 6.
    session.pointer_down(at(310.0, 310.0), None, RED).unwrap();
    session.pointer_move(at(160.0, 400.0)).unwrap();
    assert_eq!(bounds(&session, moving).x0, 150.0);

    // Within reach, but Ctrl held.
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    session.pointer_move(with(153.0, 400.0, ctrl)).unwrap();
    assert_eq!(bounds(&session, moving).x0, 143.0);
    assert_eq!(session.overlay().unwrap().guides, [None, None]);
    session.pointer_up().unwrap();

    session.set_snapping(false);
    assert!(!session.snapping());
    session.pointer_down(at(153.0, 410.0), None, RED).unwrap();
    session.pointer_move(at(154.0, 410.0)).unwrap();
    assert_eq!(bounds(&session, moving).x0, 144.0, "no snapping at all");
}

#[test]
fn an_axis_shift_locked_stays_locked() {
    let (mut session, _, moving) = two_squares();
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    // Mostly across, so Shift locks the move horizontal; the fixed square's
    // bottom (140) is near enough to the moving one's top (300 − 162 = 138)
    // to catch, were the axis free.
    session.pointer_down(at(310.0, 310.0), None, RED).unwrap();
    session.pointer_move(with(100.0, 302.0, shift)).unwrap();
    let b = bounds(&session, moving);
    assert_eq!(b.y0, 300.0, "still on its row");
}

#[test]
fn a_new_shape_catches_on_the_artboards_centre() {
    let mut session = Session::with_artboard(Size::new(800.0, 600.0));
    session.set_tool(Tool::Rect).unwrap();
    // Press near the centre (400, 300) and drag out to near (500, 403).
    session.pointer_down(at(397.0, 302.0), None, RED).unwrap();
    session.pointer_move(at(500.0, 403.0)).unwrap();
    session.pointer_up().unwrap();
    let id = session.selection().ids()[0];
    let b = bounds(&session, id);
    assert_eq!((b.x0, b.y0), (400.0, 300.0), "the press snapped");
    assert_eq!((b.x1, b.y1), (500.0, 403.0), "nothing near the far corner");
}

#[test]
fn hidden_layers_and_what_moves_are_not_caught_on() {
    let (mut session, fixed, moving) = two_squares();
    session.select_layer(fixed, false).unwrap();
    session.toggle_visible().unwrap();
    session.clear_selection().unwrap();
    session.pointer_down(at(310.0, 310.0), None, RED).unwrap();
    session.pointer_move(at(153.0, 400.0)).unwrap();
    assert_eq!(
        bounds(&session, moving).x0,
        143.0,
        "the hidden square is not there to catch"
    );
    let guides: Vec<Guide> = session
        .overlay()
        .unwrap()
        .guides
        .into_iter()
        .flatten()
        .collect();
    assert!(guides.is_empty());
}

#[test]
fn ctrl_held_at_the_press_keeps_a_new_shapes_corner_where_pressed() {
    let mut session = Session::with_artboard(Size::new(800.0, 600.0));
    session.set_tool(Tool::Rect).unwrap();
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    session
        .pointer_down(with(397.0, 302.0, ctrl), None, RED)
        .unwrap();
    session.pointer_move(with(500.0, 403.0, ctrl)).unwrap();
    session.pointer_up().unwrap();
    let id = session.selection().ids()[0];
    let b = bounds(&session, id);
    assert_eq!((b.x0, b.y0), (397.0, 302.0));
}
