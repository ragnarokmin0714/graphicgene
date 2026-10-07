//! Copy, cut, paste and duplicate: nodes travel as text, land where they
//! were copied from with fresh ids, and each paste is one undo step.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::command::Command;
use graphicgene_core::geom::{Affine, Rect, Shape, Vec2};
use graphicgene_core::node::{Node, NodeId};
use graphicgene_core::session::Session;

fn rect(session: &mut Session, name: &str, x: f64) -> NodeId {
    let path = Rect::new(x, 0.0, x + 10.0, 10.0).to_path(0.1);
    session
        .insert(Node::vector(name, path, Some(LinearRgba::BLACK)))
        .unwrap()
}

fn top(session: &Session) -> Vec<String> {
    let doc = session.document();
    doc.children_of(doc.root())
        .unwrap()
        .iter()
        .map(|&id| doc.get(id).unwrap().common.name.clone())
        .collect()
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

/// A moved and turned group at the top of the root holding one rect.
fn group_with_child(session: &mut Session) -> (NodeId, NodeId) {
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
    let transform = Affine::translate(Vec2::new(50.0, 20.0)) * Affine::rotate(0.4);
    session
        .execute(Command::SetTransform {
            id: group,
            transform,
        })
        .unwrap();
    (group, inner)
}

#[test]
fn a_paste_lands_where_the_copy_came_from_with_fresh_ids() {
    let mut session = Session::new();
    let a = rect(&mut session, "a", 0.0);
    let (group, inner) = group_with_child(&mut session);

    // A node from inside a turned group, and a whole group.
    select(&mut session, &[inner, a]);
    let text = session
        .copy_selection()
        .unwrap()
        .expect("something is selected");
    assert!(session.paste(&text).unwrap());
    assert_eq!(
        top(&session),
        ["a", "G", "a", "inner"],
        "on top, in paint order"
    );
    let pasted = session.selection().ids().to_vec();
    assert_eq!(pasted.len(), 2, "what was pasted is selected");
    assert!(
        !pasted.contains(&a) && !pasted.contains(&inner),
        "fresh ids"
    );
    assert_eq!(world(&session, pasted[0]), world(&session, a));
    assert_eq!(
        world(&session, pasted[1]),
        world(&session, inner),
        "in place"
    );

    select(&mut session, &[group]);
    let text = session.copy_selection().unwrap().unwrap();
    assert!(session.paste(&text).unwrap());
    let copy = session.selection().ids()[0];
    let children = session.document().children_of(copy).unwrap().to_vec();
    assert_eq!(children.len(), 1, "a group comes with what it holds");
    assert_ne!(children[0], inner);
    assert_eq!(world(&session, children[0]), world(&session, inner));

    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(top(&session), ["a", "G"], "each paste is one step");
    assert!(session.selection().is_empty());
    session.redo().unwrap();
    assert_eq!(top(&session), ["a", "G", "a", "inner"]);
}

#[test]
fn a_paste_can_go_into_another_document() {
    let mut source = Session::new();
    let (_, inner) = group_with_child(&mut source);
    select(&mut source, &[inner]);
    let text = source.copy_selection().unwrap().unwrap();

    let mut target = Session::new();
    assert!(target.paste(&text).unwrap());
    let pasted = target.selection().ids()[0];
    assert_eq!(world(&target, pasted), world(&source, inner));
    // And into a saved file: nothing pasted refers to the other document.
    let mut reloaded = Session::new();
    reloaded.load(&target.save().unwrap()).unwrap();
    assert_eq!(top(&reloaded), ["inner"]);
}

#[test]
fn duplicates_sit_right_above_their_originals() {
    let mut session = Session::new();
    let a = rect(&mut session, "a", 0.0);
    let b = rect(&mut session, "b", 20.0);
    rect(&mut session, "c", 40.0);
    let (group, inner) = group_with_child(&mut session);

    select(&mut session, &[b, a]);
    assert!(session.duplicate_selection().unwrap());
    assert_eq!(top(&session), ["a", "a", "b", "b", "c", "G"]);
    assert_eq!(session.selection().len(), 2, "the copies are selected");
    for &copy in session.selection().ids() {
        assert!(copy != a && copy != b);
    }

    // Inside a group, a copy stays in the group, with the same transform.
    select(&mut session, &[inner]);
    assert!(session.duplicate_selection().unwrap());
    let children = session.document().children_of(group).unwrap().to_vec();
    assert_eq!(children.len(), 2);
    assert_eq!(world(&session, children[1]), world(&session, inner));

    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(top(&session), ["a", "b", "c", "G"], "one step each");
}

#[test]
fn cut_is_a_copy_and_a_delete() {
    let mut session = Session::new();
    let a = rect(&mut session, "a", 0.0);
    rect(&mut session, "b", 20.0);
    select(&mut session, &[a]);
    let text = session.cut_selection().unwrap().unwrap();
    assert_eq!(top(&session), ["b"]);
    assert!(session.paste(&text).unwrap());
    assert_eq!(top(&session), ["b", "a"]);
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(top(&session), ["a", "b"], "the cut was one step too");

    session.clear_selection().unwrap();
    assert_eq!(session.copy_selection().unwrap(), None, "nothing to copy");
    assert_eq!(session.cut_selection().unwrap(), None);
    assert_eq!(top(&session), ["a", "b"]);
}

#[test]
fn text_that_is_not_ours_pastes_nothing() {
    let mut session = Session::new();
    let a = rect(&mut session, "a", 0.0);
    select(&mut session, &[a]);
    let text = session.copy_selection().unwrap().unwrap();

    let newer = text.replace("\"version\":1", "\"version\":2");
    let empty = r#"{"type":"graphicgene/nodes","version":1,"nodes":[]}"#;
    let other = text.replace("graphicgene/nodes", "someone/else");
    for foreign in ["hello", "", "{}", "[1, 2]", empty, &newer, &other] {
        assert!(!session.paste(foreign).unwrap(), "{foreign}");
    }
    assert_eq!(top(&session), ["a"]);

    // Well formed but inconsistent: a shape claiming children, and ids from
    // some other document. The children are dropped; the ids ignored.
    let mut tree: serde_json::Value = serde_json::from_str(&text).unwrap();
    tree["nodes"][0]["children"] = tree["nodes"].clone();
    tree["nodes"][0]["node"]["parent"] = serde_json::json!({ "idx": 7, "version": 1 });
    assert!(session.paste(&tree.to_string()).unwrap());
    assert_eq!(top(&session), ["a", "a"]);
    let pasted = session.selection().ids()[0];
    assert!(session.document().children_of(pasted).unwrap().is_empty());
}

#[test]
fn paste_in_front_and_in_back_go_right_by_the_selection() {
    use graphicgene_core::session::PastePlace;
    let mut session = Session::new();
    let a = rect(&mut session, "a", 0.0);
    rect(&mut session, "b", 20.0);
    let c = rect(&mut session, "c", 40.0);
    select(&mut session, &[a]);
    let text = session.copy_selection().unwrap().unwrap();

    select(&mut session, &[c]);
    session.paste_at(&text, PastePlace::Back).unwrap();
    assert_eq!(top(&session), ["a", "b", "a", "c"], "right below c");
    session.paste_at(&text, PastePlace::Front).unwrap();
    assert_eq!(
        top(&session),
        ["a", "b", "a", "a", "c"],
        "right above the last paste"
    );
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(top(&session), ["a", "b", "c"], "each one undo step");

    session.clear_selection().unwrap();
    session.paste_at(&text, PastePlace::Back).unwrap();
    assert_eq!(
        top(&session)[0],
        "a",
        "with nothing selected, at the bottom"
    );
    session.clear_selection().unwrap();
    session.paste_at(&text, PastePlace::Front).unwrap();
    assert_eq!(top(&session).last().unwrap(), "a", "or the top");
}

#[test]
fn pasted_into_a_turned_group_it_stays_where_it_was_copied() {
    use graphicgene_core::session::PastePlace;
    let mut session = Session::new();
    let outside = rect(&mut session, "outside", 200.0);
    let (group, inner) = group_with_child(&mut session);
    select(&mut session, &[outside]);
    let text = session.copy_selection().unwrap().unwrap();
    let before = world(&session, outside);

    select(&mut session, &[inner]);
    session.paste_at(&text, PastePlace::Front).unwrap();
    let pasted = session.selection().ids()[0];
    let doc = session.document();
    assert_eq!(
        doc.get(pasted).unwrap().common.parent,
        Some(group),
        "into the group"
    );
    let after = world(&session, pasted);
    for (x, y) in before.iter().zip(after) {
        assert!(
            (x - y).abs() < 1e-9,
            "same place on the page: {before:?} {after:?}"
        );
    }
}

/// The pointer at (x, y), with Alt held or not.
fn pointer(x: f64, y: f64, alt: bool) -> graphicgene_core::session::Pointer {
    use graphicgene_core::geom::Point;
    use graphicgene_core::gesture::Modifiers;
    graphicgene_core::session::Pointer {
        point: Point::new(x, y),
        modifiers: Modifiers {
            alt,
            ..Modifiers::default()
        },
        hit_tolerance: 4.0,
        pick_tolerance: 6.0,
    }
}

fn alt_drag(session: &mut Session, from: (f64, f64), to: (f64, f64)) {
    session
        .pointer_down(pointer(from.0, from.1, true), None, LinearRgba::BLACK)
        .unwrap();
    session.pointer_move(pointer(to.0, to.1, true)).unwrap();
    session.pointer_up().unwrap();
}

fn x0(session: &Session, id: NodeId) -> f64 {
    session.document().world_bounds(id).unwrap().x0
}

#[test]
fn an_alt_drag_moves_a_copy_as_one_undo_step() {
    let mut session = Session::new();
    session.set_snapping(false);
    let a = rect(&mut session, "a", 100.0);
    alt_drag(&mut session, (105.0, 5.0), (135.0, 5.0));
    assert_eq!(top(&session), ["a", "a"]);
    let copy = session.selection().ids()[0];
    assert_ne!(copy, a, "the copy is selected");
    assert_eq!(
        (x0(&session, a), x0(&session, copy)),
        (100.0, 130.0),
        "and moved"
    );

    session.undo().unwrap();
    assert_eq!(top(&session), ["a"], "one step takes both away");
    assert_eq!(x0(&session, a), 100.0);
    session.redo().unwrap();
    assert_eq!(x0(&session, copy), 130.0, "and redo brings both back");
}

#[test]
fn an_alt_click_or_an_abandoned_alt_drag_copies_nothing() {
    let mut session = Session::new();
    session.set_snapping(false);
    let a = rect(&mut session, "a", 100.0);
    alt_drag(&mut session, (105.0, 5.0), (105.0, 5.0));
    assert_eq!(top(&session), ["a"]);
    assert_eq!(session.selection().ids(), &[a]);

    session
        .pointer_down(pointer(105.0, 5.0, true), None, LinearRgba::BLACK)
        .unwrap();
    session.pointer_move(pointer(150.0, 5.0, true)).unwrap();
    session.escape().unwrap();
    assert_eq!(top(&session), ["a"], "Escape takes the copy away");
    assert_eq!(session.selection().ids(), &[a]);
    assert_eq!(x0(&session, a), 100.0);
}

#[test]
fn transform_again_repeats_the_last_move_and_its_copying() {
    let mut session = Session::new();
    session.set_snapping(false);
    let a = rect(&mut session, "a", 100.0);
    assert!(!session.repeat_move().unwrap(), "nothing to repeat yet");

    alt_drag(&mut session, (105.0, 5.0), (125.0, 5.0));
    assert!(session.repeat_move().unwrap());
    assert!(session.repeat_move().unwrap());
    let xs: Vec<f64> = {
        let doc = session.document();
        let ids = doc.children_of(doc.root()).unwrap().to_vec();
        ids.iter().map(|&id| x0(&session, id)).collect()
    };
    assert_eq!(
        xs,
        [100.0, 120.0, 140.0, 160.0],
        "a row of copies, 20 apart"
    );
    session.undo().unwrap();
    assert_eq!(top(&session).len(), 3, "each repeat one undo step");

    // A plain drag repeats as a plain move.
    select(&mut session, &[a]);
    session
        .pointer_down(pointer(105.0, 5.0, false), None, LinearRgba::BLACK)
        .unwrap();
    session.pointer_move(pointer(105.0, 55.0, false)).unwrap();
    session.pointer_up().unwrap();
    session.repeat_move().unwrap();
    assert_eq!(session.document().world_bounds(a).unwrap().y0, 100.0);
    assert_eq!(top(&session).len(), 3, "no copy");
}
