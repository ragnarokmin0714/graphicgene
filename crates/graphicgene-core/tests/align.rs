//! Aligning and distributing the selection: nodes only move, by their
//! boxes on the page, and each operation is one undo step.

use graphicgene_core::align::{Align, Distribute};
use graphicgene_core::color::LinearRgba;
use graphicgene_core::command::Command;
use graphicgene_core::geom::{Affine, Rect, Shape, Vec2};
use graphicgene_core::node::{Node, NodeId};
use graphicgene_core::session::Session;

fn rect(session: &mut Session, x: f64, y: f64, w: f64, h: f64) -> NodeId {
    let path = Rect::new(x, y, x + w, y + h).to_path(0.1);
    session
        .insert(Node::vector("r", path, Some(LinearRgba::BLACK)))
        .unwrap()
}

fn select(session: &mut Session, ids: &[NodeId]) {
    session.clear_selection().unwrap();
    for &id in ids {
        session.select_layer(id, true).unwrap();
    }
}

fn bounds(session: &Session, id: NodeId) -> Rect {
    session.document().world_bounds(id).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn several_nodes_line_up_with_each_other_in_one_step() {
    let mut session = Session::new();
    let a = rect(&mut session, 10.0, 0.0, 20.0, 20.0);
    let b = rect(&mut session, 50.0, 40.0, 40.0, 10.0);
    let c = rect(&mut session, 30.0, 80.0, 10.0, 30.0);
    select(&mut session, &[a, b, c]);

    assert!(session.align_selection(Align::Left).unwrap());
    for id in [a, b, c] {
        assert!(close(bounds(&session, id).x0, 10.0));
    }
    // Only across: nothing moved down.
    assert!(close(bounds(&session, c).y0, 80.0));

    assert!(session.align_selection(Align::Bottom).unwrap());
    for id in [a, b, c] {
        assert!(close(bounds(&session, id).y1, 110.0));
    }

    // The combined box now spans 10..50 across; centres meet at 30.
    assert!(session.align_selection(Align::CenterX).unwrap());
    for id in [a, b, c] {
        assert!(close(bounds(&session, id).center().x, 30.0));
    }

    session.undo().unwrap();
    session.undo().unwrap();
    assert!(close(bounds(&session, a).y0, 0.0), "each was one undo step");
    assert!(close(bounds(&session, b).x0, 10.0));
}

#[test]
fn one_node_lines_up_with_the_artboard() {
    let mut session = Session::new();
    let size = session.document().artboard();
    let a = rect(&mut session, 10.0, 20.0, 40.0, 30.0);
    select(&mut session, &[a]);
    assert!(session.align_selection(Align::CenterX).unwrap());
    assert!(session.align_selection(Align::CenterY).unwrap());
    let b = bounds(&session, a);
    assert!(close(b.center().x, size.width / 2.0) && close(b.center().y, size.height / 2.0));
    assert!(session.align_selection(Align::Right).unwrap());
    assert!(close(bounds(&session, a).x1, size.width));
}

#[test]
fn nothing_to_move_is_no_undo_step() {
    let mut session = Session::new();
    let a = rect(&mut session, 0.0, 0.0, 10.0, 10.0);
    let b = rect(&mut session, 0.0, 30.0, 20.0, 10.0);
    select(&mut session, &[a, b]);
    let undoable = session.can_undo();
    assert!(!session.align_selection(Align::Left).unwrap());
    assert_eq!(session.can_undo(), undoable);
    session.clear_selection().unwrap();
    assert!(
        !session.align_selection(Align::Left).unwrap(),
        "nothing selected"
    );
}

#[test]
fn distributing_evens_the_gaps_and_keeps_the_ends() {
    let mut session = Session::new();
    // Gaps of 0 and 70 between boxes 10, 20 and 10 wide over 0..110.
    let first = rect(&mut session, 0.0, 0.0, 10.0, 10.0);
    let middle = rect(&mut session, 10.0, 5.0, 20.0, 10.0);
    let last = rect(&mut session, 100.0, 0.0, 10.0, 10.0);
    // Selected out of order: neighbours go by position, not selection.
    select(&mut session, &[last, first, middle]);

    assert!(
        session
            .distribute_selection(Distribute::Horizontal)
            .unwrap()
    );
    assert!(close(bounds(&session, first).x0, 0.0));
    assert!(close(bounds(&session, last).x0, 100.0));
    // (110 − 40) / 2 = 35 between each pair.
    assert!(close(bounds(&session, middle).x0, 45.0));
    assert!(close(bounds(&session, middle).y0, 5.0), "only across");

    assert!(
        !session
            .distribute_selection(Distribute::Horizontal)
            .unwrap(),
        "already even"
    );
    select(&mut session, &[first, middle]);
    assert!(
        !session.distribute_selection(Distribute::Vertical).unwrap(),
        "two cannot be spaced"
    );
}

#[test]
fn a_node_inside_a_turned_group_aligns_by_its_box_on_the_page() {
    let mut session = Session::new();
    let root = session.document().root();
    session
        .execute(Command::InsertNode {
            parent: root,
            index: 0,
            node: Node::group("G"),
        })
        .unwrap();
    let group = session.document().children_of(root).unwrap()[0];
    let path = Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1);
    session
        .execute(Command::InsertNode {
            parent: group,
            index: 0,
            node: Node::vector("inner", path, Some(LinearRgba::BLACK)),
        })
        .unwrap();
    let inner = session.document().children_of(group).unwrap()[0];
    let turned =
        Affine::translate(Vec2::new(100.0, 50.0)) * Affine::rotate(0.5) * Affine::scale(2.0);
    session
        .execute(Command::SetTransform {
            id: group,
            transform: turned,
        })
        .unwrap();
    let other = rect(&mut session, 300.0, 0.0, 10.0, 10.0);

    select(&mut session, &[inner, other]);
    let before = bounds(&session, inner);
    assert!(session.align_selection(Align::Right).unwrap());
    let after = bounds(&session, inner);
    assert!(close(after.x1, 310.0), "its box reaches the other's edge");
    assert!(close(after.width(), before.width()) && close(after.y0, before.y0));
    assert_eq!(
        session
            .document()
            .get(group)
            .unwrap()
            .common
            .transform
            .as_coeffs(),
        turned.as_coeffs(),
        "the group stays put"
    );
}

#[test]
fn a_group_and_its_child_move_as_the_group() {
    let mut session = Session::new();
    let a = rect(&mut session, 0.0, 0.0, 10.0, 10.0);
    let b = rect(&mut session, 20.0, 0.0, 10.0, 10.0);
    select(&mut session, &[a, b]);
    assert!(session.group_selection().unwrap());
    let group = session.selection().ids()[0];
    let c = rect(&mut session, 100.0, 50.0, 10.0, 10.0);

    // The group moves down 50 to meet c's bottom; a, inside it, must not
    // move a second time on its own.
    select(&mut session, &[group, a, c]);
    assert!(session.align_selection(Align::Bottom).unwrap());
    assert!(
        close(bounds(&session, a).y1, 60.0),
        "moved once, with the group"
    );
    assert!(close(bounds(&session, b).y1, 60.0));
    assert!(close(bounds(&session, c).y1, 60.0));
}
