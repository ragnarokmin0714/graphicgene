//! Layer-panel operations: renaming, hiding and locking, moving layers,
//! stepping them through the stacking order, grouping and ungrouping. Each
//! is one undo step, and nothing moves on the page unless asked to.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::command::Command;
use graphicgene_core::geom::{Affine, Point, Rect, Shape, Vec2};
use graphicgene_core::gesture::TransformKind;
use graphicgene_core::layers::{Arrange, Drop};
use graphicgene_core::node::{Node, NodeId};
use graphicgene_core::properties::Property;
use graphicgene_core::session::{SelectOutcome, Session};

fn rect(session: &mut Session, name: &str, x: f64) -> NodeId {
    let path = Rect::new(x, 0.0, x + 10.0, 10.0).to_path(0.1);
    session
        .insert(Node::vector(name, path, Some(LinearRgba::BLACK)))
        .unwrap()
}

/// Names of `parent`'s children in paint order, bottom first.
fn order(session: &Session, parent: NodeId) -> Vec<String> {
    let doc = session.document();
    doc.children_of(parent)
        .unwrap()
        .iter()
        .map(|&id| doc.get(id).unwrap().common.name.clone())
        .collect()
}

fn top(session: &Session) -> Vec<String> {
    order(session, session.document().root())
}

fn select(session: &mut Session, ids: &[NodeId]) {
    session.clear_selection().unwrap();
    for &id in ids {
        session.select_layer(id, true).unwrap();
    }
}

fn world(session: &Session, id: NodeId) -> [f64; 6] {
    session.document().world_transform(id).unwrap().as_coeffs()
}

fn assert_same_place(a: [f64; 6], b: [f64; 6]) {
    assert!(
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9),
        "{a:?} != {b:?}"
    );
}

/// A group at the top of the root, moved and scaled, holding one rect.
fn transformed_group(session: &mut Session) -> (NodeId, NodeId) {
    let root = session.document().root();
    let index = session.document().children_of(root).unwrap().len();
    session
        .execute(Command::InsertNode {
            parent: root,
            index,
            node: Node::group("G"),
        })
        .unwrap();
    let group = session.document().children_of(root).unwrap()[index];
    let path = Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1);
    session
        .execute(Command::InsertNode {
            parent: group,
            index: 0,
            node: Node::vector("inner", path, Some(LinearRgba::BLACK)),
        })
        .unwrap();
    let inner = session.document().children_of(group).unwrap()[0];
    let transform =
        Affine::translate(Vec2::new(100.0, 50.0)) * Affine::rotate(0.5) * Affine::scale(2.0);
    session
        .execute(Command::SetTransform {
            id: group,
            transform,
        })
        .unwrap();
    (group, inner)
}

#[test]
fn renaming_trims_and_skips_what_changes_nothing() {
    let mut session = Session::new();
    let a = rect(&mut session, "a", 0.0);
    assert!(session.rename(a, "  Logo ").unwrap());
    assert_eq!(top(&session), ["Logo"]);
    assert!(!session.rename(a, "Logo").unwrap(), "the name it has");
    assert!(!session.rename(a, "   ").unwrap(), "blank");
    session.undo().unwrap();
    assert_eq!(top(&session), ["a"], "one undo step");
}

#[test]
fn hiding_and_locking_toggle_the_selection_as_one_step() {
    let mut session = Session::new();
    let a = rect(&mut session, "a", 0.0);
    let b = rect(&mut session, "b", 20.0);
    let hidden = |s: &Session, id| !s.document().get(id).unwrap().common.visible;

    select(&mut session, &[a, b]);
    assert!(session.toggle_visible().unwrap());
    assert!(hidden(&session, a) && hidden(&session, b));
    assert!(session.toggle_visible().unwrap(), "all hidden: show them");
    assert!(!hidden(&session, a) && !hidden(&session, b));
    session.set_visible(&[a], false).unwrap();
    assert!(
        session.toggle_visible().unwrap(),
        "some hidden: hide the rest"
    );
    assert!(hidden(&session, a) && hidden(&session, b));
    assert!(!session.set_visible(&[a], false).unwrap(), "already hidden");
    session.undo().unwrap();
    assert!(!hidden(&session, b) && hidden(&session, a), "one step");

    // Locked: selectable from the panel, untouchable on the canvas.
    select(&mut session, &[b]);
    assert!(session.toggle_locked().unwrap());
    assert!(session.overlay().unwrap().locked);
    let inside = Point::new(25.0, 5.0);
    assert!(
        !session
            .begin_transform(TransformKind::Move, inside)
            .unwrap()
    );
    assert!(!session.nudge(Vec2::new(1.0, 0.0)).unwrap());
    assert!(!session.begin_path_edit().unwrap());
    assert!(
        session.set_property(Property::X(40.0)).unwrap(),
        "the panel still can"
    );
    session.clear_selection().unwrap();
    assert_eq!(
        session.select_at(inside, false, 1.0).unwrap(),
        SelectOutcome::Miss
    );
    session.select_all().unwrap();
    assert!(session.selection().is_empty(), "a is hidden, b locked");
    select(&mut session, &[b]);
    assert!(
        session.delete_selection().unwrap(),
        "deleting is still allowed"
    );
}

#[test]
fn moving_layers_keeps_their_order_and_their_place_on_the_page() {
    let mut session = Session::new();
    let a = rect(&mut session, "a", 0.0);
    let b = rect(&mut session, "b", 20.0);
    let c = rect(&mut session, "c", 40.0);
    let d = rect(&mut session, "d", 60.0);

    select(&mut session, &[a]);
    assert!(session.move_selection(Drop::Above(c)).unwrap());
    assert_eq!(top(&session), ["b", "c", "a", "d"]);
    // Several at once keep their order among themselves, whatever order
    // they were selected in.
    select(&mut session, &[d, b]);
    assert!(session.move_selection(Drop::Below(c)).unwrap());
    assert_eq!(top(&session), ["b", "d", "c", "a"]);
    session.undo().unwrap();
    assert_eq!(top(&session), ["b", "c", "a", "d"], "one step");

    // Into a transformed group and out again, without moving on the page.
    let (group, _) = transformed_group(&mut session);
    let before = world(&session, a);
    select(&mut session, &[a]);
    assert!(session.move_selection(Drop::Inside(group)).unwrap());
    assert_eq!(
        order(&session, group),
        ["inner", "a"],
        "on top of the group"
    );
    assert_same_place(world(&session, a), before);
    assert!(session.move_selection(Drop::Below(b)).unwrap());
    assert_eq!(top(&session), ["a", "b", "c", "d", "G"]);
    assert_same_place(world(&session, a), before);
}

#[test]
fn moves_that_change_nothing_or_cannot_be_made_record_nothing() {
    let mut session = Session::new();
    let a = rect(&mut session, "a", 0.0);
    let b = rect(&mut session, "b", 20.0);
    let (group, inner) = transformed_group(&mut session);

    select(&mut session, &[b]);
    let version = session.layers_version();
    assert!(
        !session.move_selection(Drop::Above(b)).unwrap(),
        "next to itself"
    );
    assert!(
        !session.move_selection(Drop::Above(a)).unwrap(),
        "where it is"
    );
    assert!(
        !session.move_selection(Drop::Inside(a)).unwrap(),
        "into a shape"
    );
    assert_eq!(
        session.layers_version(),
        version,
        "nothing reached the journal"
    );

    select(&mut session, &[group]);
    let version = session.layers_version();
    assert!(
        !session.move_selection(Drop::Inside(group)).unwrap(),
        "into itself"
    );
    assert!(
        !session.move_selection(Drop::Above(inner)).unwrap(),
        "into its own subtree"
    );
    assert_eq!(session.layers_version(), version);
}

#[test]
fn arranging_steps_past_siblings_within_their_parent() {
    let mut session = Session::new();
    let ids: Vec<NodeId> = ["a", "b", "c", "d"]
        .iter()
        .enumerate()
        .map(|(i, name)| rect(&mut session, name, i as f64 * 20.0))
        .collect();
    select(&mut session, &[ids[0], ids[2]]);

    assert!(session.arrange(Arrange::Forward).unwrap());
    assert_eq!(top(&session), ["b", "a", "d", "c"]);
    assert!(session.arrange(Arrange::Backward).unwrap());
    assert_eq!(top(&session), ["a", "b", "c", "d"]);
    assert!(session.arrange(Arrange::ToFront).unwrap());
    assert_eq!(top(&session), ["b", "d", "a", "c"]);
    assert!(
        !session.arrange(Arrange::Forward).unwrap(),
        "already on top"
    );
    assert!(session.arrange(Arrange::ToBack).unwrap());
    assert_eq!(top(&session), ["a", "c", "b", "d"]);
    session.undo().unwrap();
    assert_eq!(top(&session), ["b", "d", "a", "c"], "one step each");
}

#[test]
fn grouping_takes_the_topmost_place_and_moves_nothing_on_the_page() {
    let mut session = Session::new();
    let a = rect(&mut session, "a", 0.0);
    rect(&mut session, "b", 20.0);
    let c = rect(&mut session, "c", 40.0);
    rect(&mut session, "d", 60.0);
    let places = [world(&session, a), world(&session, c)];

    select(&mut session, &[c, a]);
    assert!(session.group_selection().unwrap());
    let group = session.selection().ids()[0];
    assert_eq!(session.selection().len(), 1, "the new group is selected");
    assert_eq!(top(&session), ["b", "Group", "d"], "where c was");
    assert_eq!(order(&session, group), ["a", "c"], "in their own order");
    for (id, place) in [a, c].into_iter().zip(places) {
        assert_same_place(world(&session, id), place);
    }
    session.undo().unwrap();
    assert_eq!(top(&session), ["a", "b", "c", "d"], "one step");
    assert!(session.selection().is_empty(), "undo took the group away");
    session.redo().unwrap();
    assert_eq!(top(&session), ["b", "Group", "d"]);

    // From two parents, the root and a transformed group: the new group
    // goes where the topmost of them was, inside that group, and still
    // nothing moves on the page.
    let (g, inner) = transformed_group(&mut session);
    let places = [world(&session, a), world(&session, inner)];
    select(&mut session, &[inner, a]);
    assert!(session.group_selection().unwrap());
    let outer = session.selection().ids()[0];
    assert_eq!(top(&session), ["b", "Group", "d", "G"]);
    assert_eq!(order(&session, g), ["Group"]);
    assert_eq!(order(&session, outer), ["a", "inner"]);
    for (id, place) in [a, inner].into_iter().zip(places) {
        assert_same_place(world(&session, id), place);
    }
}

#[test]
fn ungrouping_folds_the_group_into_what_it_held() {
    let mut session = Session::new();
    rect(&mut session, "a", 0.0);
    let (group, inner) = transformed_group(&mut session);
    rect(&mut session, "z", 90.0);
    let place = world(&session, inner);
    session
        .execute(Command::SetOpacity {
            id: group,
            opacity: 0.5,
        })
        .unwrap();
    session.set_visible(&[group], false).unwrap();

    select(&mut session, &[group]);
    assert!(session.ungroup_selection().unwrap());
    assert_eq!(top(&session), ["a", "inner", "z"], "in the group's place");
    assert_eq!(session.selection().ids(), [inner]);
    let node = &session.document().get(inner).unwrap().common;
    assert_eq!((node.opacity, node.visible), (0.5, false));
    assert_same_place(world(&session, inner), place);

    session.undo().unwrap();
    assert_eq!(top(&session), ["a", "G", "z"], "one step");
    assert_eq!(order(&session, group), ["inner"]);
    assert_same_place(world(&session, inner), place);
    select(&mut session, &[inner]);
    assert!(!session.ungroup_selection().unwrap(), "not a group");
}
