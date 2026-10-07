//! Selection, hit-testing and drag gestures.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::command::{Command, Journal};
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Affine, Point, Rect, Shape};
use graphicgene_core::gesture::{Frame, Gesture, Modifiers, ShapeKind, TransformKind};
use graphicgene_core::hit;
use graphicgene_core::node::{Node, NodeId};
use graphicgene_core::selection::Selection;

const NONE: Modifiers = Modifiers {
    shift: false,
    alt: false,
    ctrl: false,
};
const SHIFT: Modifiers = Modifiers {
    shift: true,
    alt: false,
    ctrl: false,
};
const ALT: Modifiers = Modifiers {
    shift: false,
    alt: true,
    ctrl: false,
};

/// A filled square with its top-left corner at (x, y).
fn square(doc: &mut Document, journal: &mut Journal, x: f64, y: f64, size: f64) -> NodeId {
    let node = Node::vector(
        "Square",
        Rect::new(x, y, x + size, y + size).to_path(0.1),
        Some(LinearRgba::BLACK),
    );
    let parent = doc.root();
    let index = doc.children_of(parent).unwrap().len();
    journal
        .execute(
            doc,
            Command::InsertNode {
                parent,
                index,
                node,
            },
        )
        .unwrap();
    *doc.children_of(parent).unwrap().last().unwrap()
}

fn world_bounds(doc: &Document, id: NodeId) -> Rect {
    let b = doc.world_bounds(id).unwrap();
    // Round away float noise so assertions can compare exact numbers.
    let r = |v: f64| (v * 1e6).round() / 1e6;
    Rect::new(r(b.x0), r(b.y0), r(b.x1), r(b.y1))
}

fn drag(
    doc: &mut Document,
    journal: &mut Journal,
    selection: &mut Selection,
    kind: TransformKind,
    from: (f64, f64),
    to: (f64, f64),
    modifiers: Modifiers,
) {
    let mut gesture = Gesture::transform(doc, selection, kind, from.into(), false)
        .unwrap()
        .expect("selection has a frame");
    gesture
        .update(doc, selection, to.into(), modifiers, 0.0)
        .unwrap();
    gesture.commit(doc, journal, selection).unwrap();
}

#[test]
fn hit_test_finds_the_topmost_node_and_respects_transforms() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let below = square(&mut doc, &mut journal, 0.0, 0.0, 20.0);
    let above = square(&mut doc, &mut journal, 10.0, 10.0, 20.0);

    assert_eq!(
        hit::hit_test(&doc, Point::new(15.0, 15.0), 0.0).unwrap(),
        Some(above)
    );
    assert_eq!(
        hit::hit_test(&doc, Point::new(5.0, 5.0), 0.0).unwrap(),
        Some(below)
    );
    assert_eq!(
        hit::hit_test(&doc, Point::new(50.0, 50.0), 0.0).unwrap(),
        None
    );
    // Tolerance reaches just outside an edge.
    assert_eq!(
        hit::hit_test(&doc, Point::new(33.0, 20.0), 4.0).unwrap(),
        Some(above)
    );

    // Moving the node moves what is hit: the test runs in local space.
    journal
        .execute(
            &mut doc,
            Command::SetTransform {
                id: above,
                transform: Affine::translate((100.0, 0.0)),
            },
        )
        .unwrap();
    assert_eq!(
        hit::hit_test(&doc, Point::new(15.0, 15.0), 0.0).unwrap(),
        Some(below)
    );
    assert_eq!(
        hit::hit_test(&doc, Point::new(115.0, 15.0), 0.0).unwrap(),
        Some(above)
    );
}

#[test]
fn hidden_and_locked_nodes_are_not_hit() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = square(&mut doc, &mut journal, 0.0, 0.0, 20.0);
    doc.get_mut(id).unwrap().common.locked = true;
    assert_eq!(
        hit::hit_test(&doc, Point::new(5.0, 5.0), 0.0).unwrap(),
        None
    );
    doc.get_mut(id).unwrap().common.locked = false;
    doc.get_mut(id).unwrap().common.visible = false;
    assert_eq!(
        hit::hit_test(&doc, Point::new(5.0, 5.0), 0.0).unwrap(),
        None
    );
}

#[test]
fn click_selection_rules() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let a = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let b = square(&mut doc, &mut journal, 20.0, 0.0, 10.0);
    let mut selection = Selection::new();

    assert!(selection.click(Some(a), false));
    assert_eq!(selection.ids(), [a]);
    // Shift adds; clicking an already-selected node keeps the group so it
    // can be dragged together.
    assert!(selection.click(Some(b), true));
    assert!(selection.click(Some(a), false));
    assert_eq!(selection.ids(), [a, b]);
    // Shift on a selected node deselects it, and must not start a drag.
    assert!(!selection.click(Some(a), true));
    assert_eq!(selection.ids(), [b]);
    // Empty space clears, unless Shift is held.
    assert!(!selection.click(None, true));
    assert_eq!(selection.ids(), [b]);
    assert!(!selection.click(None, false));
    assert!(selection.is_empty());
}

#[test]
fn a_whole_drag_is_one_undo_step() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let mut selection = Selection::new();
    selection.set([id]);

    let mut gesture = Gesture::transform(
        &doc,
        &selection,
        TransformKind::Move,
        Point::new(5.0, 5.0),
        false,
    )
    .unwrap()
    .unwrap();
    for step in 1..=50 {
        let p = Point::new(5.0 + step as f64, 5.0);
        gesture
            .update(&mut doc, &mut selection, p, NONE, 0.0)
            .unwrap();
    }
    gesture
        .commit(&mut doc, &mut journal, &mut selection)
        .unwrap();
    assert_eq!(world_bounds(&doc, id), Rect::new(50.0, 0.0, 60.0, 10.0));

    journal.undo(&mut doc).unwrap();
    assert_eq!(world_bounds(&doc, id), Rect::new(0.0, 0.0, 10.0, 10.0));
    // The next undo is the insert, not an intermediate drag position.
    journal.undo(&mut doc).unwrap();
    assert!(!doc.is_attached(id));
}

#[test]
fn shift_locks_a_move_to_the_dominant_axis() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let mut selection = Selection::new();
    selection.set([id]);
    drag(
        &mut doc,
        &mut journal,
        &mut selection,
        TransformKind::Move,
        (0.0, 0.0),
        (30.0, 4.0),
        SHIFT,
    );
    assert_eq!(world_bounds(&doc, id), Rect::new(30.0, 0.0, 40.0, 10.0));
}

#[test]
fn a_corner_handle_scales_about_the_opposite_corner() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = square(&mut doc, &mut journal, 10.0, 10.0, 10.0);
    let mut selection = Selection::new();
    selection.set([id]);

    // Drag the bottom-right handle from (20, 20) to (30, 40): the top-left
    // corner stays put.
    let kind = TransformKind::Scale { u: 1.0, v: 1.0 };
    drag(
        &mut doc,
        &mut journal,
        &mut selection,
        kind,
        (20.0, 20.0),
        (30.0, 40.0),
        NONE,
    );
    assert_eq!(world_bounds(&doc, id), Rect::new(10.0, 10.0, 30.0, 40.0));
}

#[test]
fn shift_keeps_proportions_and_alt_scales_from_the_centre() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let mut selection = Selection::new();
    selection.set([id]);

    let corner = TransformKind::Scale { u: 1.0, v: 1.0 };
    drag(
        &mut doc,
        &mut journal,
        &mut selection,
        corner,
        (10.0, 10.0),
        (30.0, 15.0),
        SHIFT,
    );
    assert_eq!(world_bounds(&doc, id), Rect::new(0.0, 0.0, 30.0, 30.0));

    journal.undo(&mut doc).unwrap();
    // Right-edge handle with Alt: grows symmetrically around x = 5.
    let edge = TransformKind::Scale { u: 1.0, v: 0.5 };
    drag(
        &mut doc,
        &mut journal,
        &mut selection,
        edge,
        (10.0, 5.0),
        (15.0, 5.0),
        ALT,
    );
    assert_eq!(world_bounds(&doc, id), Rect::new(-5.0, 0.0, 15.0, 10.0));
}

#[test]
fn scaling_through_zero_flips_instead_of_collapsing() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let mut selection = Selection::new();
    selection.set([id]);

    let right = TransformKind::Scale { u: 1.0, v: 0.5 };
    drag(
        &mut doc,
        &mut journal,
        &mut selection,
        right,
        (10.0, 5.0),
        (0.0, 5.0),
        NONE,
    );
    // Dragged exactly onto the anchor: a sliver, but still invertible.
    let t = doc.get(id).unwrap().common.transform;
    assert!(t.determinant().abs() > 0.0);

    journal.undo(&mut doc).unwrap();
    drag(
        &mut doc,
        &mut journal,
        &mut selection,
        right,
        (10.0, 5.0),
        (-10.0, 5.0),
        NONE,
    );
    assert_eq!(world_bounds(&doc, id), Rect::new(-10.0, 0.0, 0.0, 10.0));
}

#[test]
fn rotation_turns_about_the_centre_and_shift_snaps() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let mut selection = Selection::new();
    selection.set([id]);

    // From straight right of the centre (5, 5) to straight below it: 90°.
    drag(
        &mut doc,
        &mut journal,
        &mut selection,
        TransformKind::Rotate,
        (15.0, 5.0),
        (5.0, 15.0),
        NONE,
    );
    let [a, b, ..] = doc.get(id).unwrap().common.transform.as_coeffs();
    assert!((b.atan2(a) - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    assert_eq!(world_bounds(&doc, id), Rect::new(0.0, 0.0, 10.0, 10.0));

    journal.undo(&mut doc).unwrap();
    // 20° off the start with Shift snaps to 15°.
    let angle = 20f64.to_radians();
    let to = (5.0 + 10.0 * angle.cos(), 5.0 + 10.0 * angle.sin());
    drag(
        &mut doc,
        &mut journal,
        &mut selection,
        TransformKind::Rotate,
        (15.0, 5.0),
        to,
        SHIFT,
    );
    let [a, b, ..] = doc.get(id).unwrap().common.transform.as_coeffs();
    assert!((b.atan2(a) - 15f64.to_radians()).abs() < 1e-9);
}

#[test]
fn a_single_rotated_node_keeps_an_oriented_frame() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let mut selection = Selection::new();
    selection.set([id]);
    drag(
        &mut doc,
        &mut journal,
        &mut selection,
        TransformKind::Rotate,
        (15.0, 5.0),
        (5.0, 15.0),
        NONE,
    );

    let frame = Frame::of(&doc, selection.ids()).unwrap().unwrap();
    let (w, h) = frame.size();
    // The frame is the square itself, not the (equal here, but in general
    // larger) axis-aligned box around it.
    assert!((w - 10.0).abs() < 1e-9 && (h - 10.0).abs() < 1e-9);
    assert_ne!(frame.transform, Affine::IDENTITY);
}

#[test]
fn cancel_restores_transforms_and_selection() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let mut selection = Selection::new();
    selection.set([id]);

    let mut gesture = Gesture::transform(
        &doc,
        &selection,
        TransformKind::Move,
        Point::new(0.0, 0.0),
        false,
    )
    .unwrap()
    .unwrap();
    gesture
        .update(&mut doc, &mut selection, Point::new(40.0, 40.0), NONE, 0.0)
        .unwrap();
    gesture.cancel(&mut doc, &mut selection).unwrap();
    assert_eq!(world_bounds(&doc, id), Rect::new(0.0, 0.0, 10.0, 10.0));
    assert_eq!(selection.ids(), [id]);
    // Only the insert is in the journal.
    journal.undo(&mut doc).unwrap();
    assert!(!journal.can_undo());
}

#[test]
fn drawing_a_shape_is_one_undoable_insert() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let mut selection = Selection::new();

    let mut gesture = Gesture::create(
        &mut doc,
        &mut selection,
        ShapeKind::Rect,
        LinearRgba::BLACK,
        Point::new(10.0, 10.0),
        None,
    )
    .unwrap();
    gesture
        .update(&mut doc, &mut selection, Point::new(40.0, 30.0), NONE, 0.0)
        .unwrap();
    let id = gesture
        .commit(&mut doc, &mut journal, &mut selection)
        .unwrap()
        .unwrap();

    assert_eq!(world_bounds(&doc, id), Rect::new(10.0, 10.0, 40.0, 30.0));
    assert_eq!(selection.ids(), [id]);

    journal.undo(&mut doc).unwrap();
    assert!(!doc.is_attached(id));
    assert!(!journal.can_undo());
    journal.redo(&mut doc).unwrap();
    // Same id, same geometry.
    assert_eq!(world_bounds(&doc, id), Rect::new(10.0, 10.0, 40.0, 30.0));
}

#[test]
fn shape_modifiers_and_click_to_place() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let mut selection = Selection::new();
    let mut draw = |from: (f64, f64), to: (f64, f64), modifiers| {
        let mut gesture = Gesture::create(
            &mut doc,
            &mut selection,
            ShapeKind::Ellipse,
            LinearRgba::BLACK,
            from.into(),
            None,
        )
        .unwrap();
        gesture
            .update(&mut doc, &mut selection, to.into(), modifiers, 0.0)
            .unwrap();
        let id = gesture
            .commit(&mut doc, &mut journal, &mut selection)
            .unwrap()
            .unwrap();
        world_bounds(&doc, id)
    };

    // Shift: a circle, sized by the longer side, towards the pointer.
    assert_eq!(
        draw((50.0, 50.0), (20.0, 40.0), SHIFT),
        Rect::new(20.0, 20.0, 50.0, 50.0)
    );
    // Alt: centred on the press.
    assert_eq!(
        draw((50.0, 50.0), (60.0, 55.0), ALT),
        Rect::new(40.0, 45.0, 60.0, 55.0)
    );
    // A click places a default-sized shape at the press.
    assert_eq!(
        draw((5.0, 5.0), (5.5, 5.0), NONE),
        Rect::new(5.0, 5.0, 105.0, 105.0)
    );
}

#[test]
fn cancelling_a_draw_leaves_no_trace() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let existing = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let mut selection = Selection::new();
    selection.set([existing]);

    let gesture = Gesture::create(
        &mut doc,
        &mut selection,
        ShapeKind::Rect,
        LinearRgba::BLACK,
        Point::new(20.0, 20.0),
        None,
    )
    .unwrap();
    gesture.cancel(&mut doc, &mut selection).unwrap();

    assert_eq!(doc.children_of(doc.root()).unwrap(), [existing]);
    assert_eq!(selection.ids(), [existing]);
}

#[test]
fn marquee_selects_what_it_touches() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let a = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let b = square(&mut doc, &mut journal, 20.0, 0.0, 10.0);
    let c = square(&mut doc, &mut journal, 40.0, 0.0, 10.0);
    let mut selection = Selection::new();

    let mut gesture = Gesture::marquee(&selection, Point::new(5.0, -5.0), false);
    gesture
        .update(&mut doc, &mut selection, Point::new(25.0, 5.0), NONE, 0.0)
        .unwrap();
    assert_eq!(selection.ids(), [a, b]);
    assert_eq!(
        gesture.marquee_rect(),
        Some(Rect::new(5.0, -5.0, 25.0, 5.0))
    );
    gesture
        .commit(&mut doc, &mut journal, &mut selection)
        .unwrap();

    // Additive: keeps what was selected at the press.
    let mut gesture = Gesture::marquee(&selection, Point::new(45.0, 5.0), true);
    gesture
        .update(&mut doc, &mut selection, Point::new(46.0, 6.0), NONE, 0.0)
        .unwrap();
    assert_eq!(selection.ids(), [a, b, c]);
}

#[test]
fn a_multi_selection_moves_together() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let a = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let b = square(&mut doc, &mut journal, 20.0, 0.0, 10.0);
    let mut selection = Selection::new();
    selection.set([a, b]);

    drag(
        &mut doc,
        &mut journal,
        &mut selection,
        TransformKind::Move,
        (0.0, 0.0),
        (0.0, 10.0),
        NONE,
    );
    assert_eq!(world_bounds(&doc, a), Rect::new(0.0, 10.0, 10.0, 20.0));
    assert_eq!(world_bounds(&doc, b), Rect::new(20.0, 10.0, 30.0, 20.0));
    journal.undo(&mut doc).unwrap();
    assert_eq!(world_bounds(&doc, a), Rect::new(0.0, 0.0, 10.0, 10.0));
    assert_eq!(world_bounds(&doc, b), Rect::new(20.0, 0.0, 30.0, 10.0));
}

#[test]
fn a_failing_batch_leaves_the_document_untouched() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let a = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let group = doc.insert_detached(Node::group("Detached"));
    let before = doc.get(a).unwrap().common.transform;

    // The second command fails (the group is not attached anywhere, so it
    // cannot be detached); the first must be rolled back.
    let result = journal.execute(
        &mut doc,
        Command::Batch(vec![
            Command::SetTransform {
                id: a,
                transform: Affine::translate((5.0, 5.0)),
            },
            Command::Detach { id: group },
        ]),
    );
    assert!(result.is_err());
    assert_eq!(doc.get(a).unwrap().common.transform, before);
}

#[test]
fn selection_drops_nodes_that_leave_the_tree() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let a = square(&mut doc, &mut journal, 0.0, 0.0, 10.0);
    let mut selection = Selection::new();
    selection.set([a]);
    journal.undo(&mut doc).unwrap();
    selection.retain_attached(&doc);
    assert!(selection.is_empty());
}
