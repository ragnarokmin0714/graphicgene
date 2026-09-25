//! The anchor model, the pen tool and path editing.

use graphicgene_core::anchors::{AnchorId, AnchorPath, HandleSide, PathHit};
use graphicgene_core::color::LinearRgba;
use graphicgene_core::command::{Command, Journal};
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Affine, BezPath, Ellipse, Point, Rect, Shape};
use graphicgene_core::gesture::Modifiers;
use graphicgene_core::node::{Node, NodeId, Stroke};
use graphicgene_core::path_edit::{DeleteOutcome, PathEdit, PressOutcome};
use graphicgene_core::pen::PenSession;
use graphicgene_core::selection::Selection;

const NONE: Modifiers = Modifiers {
    shift: false,
    alt: false,
};
const SHIFT: Modifiers = Modifiers {
    shift: true,
    alt: false,
};
const ALT: Modifiers = Modifiers {
    shift: false,
    alt: true,
};
const STROKE: Stroke = Stroke {
    color: LinearRgba::BLACK,
    width: 2.0,
};

fn id(subpath: usize, index: usize) -> AnchorId {
    AnchorId { subpath, index }
}

fn close(a: Point, b: Point) -> bool {
    (a - b).hypot() < 1e-6
}

fn insert(doc: &mut Document, journal: &mut Journal, path: BezPath) -> NodeId {
    let parent = doc.root();
    let index = doc.children_of(parent).unwrap().len();
    let node = Node::vector("Shape", path, Some(LinearRgba::BLACK));
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

fn anchors_of(doc: &Document, id: NodeId) -> AnchorPath {
    AnchorPath::from_bez(doc.vector_path(id).unwrap())
}

// ---- Anchor model --------------------------------------------------------

#[test]
fn a_rectangle_is_four_corner_anchors() {
    let anchors = AnchorPath::from_bez(&Rect::new(0.0, 0.0, 10.0, 20.0).to_path(0.1));
    assert_eq!(anchors.subpaths.len(), 1);
    assert!(anchors.subpaths[0].closed);
    assert_eq!(anchors.len(), 4);
    assert!(
        anchors
            .ids()
            .all(|i| anchors.get(i).unwrap().handle_in.is_none())
    );
}

#[test]
fn an_ellipse_is_four_smooth_anchors_and_round_trips() {
    // kurbo draws the ellipse's closing curve back onto the start point;
    // that must come back as one anchor, not a duplicate.
    let original = Ellipse::new((50.0, 50.0), (40.0, 20.0), 0.0).to_path(0.1);
    let anchors = AnchorPath::from_bez(&original);
    assert_eq!(anchors.len(), 4);
    assert!(anchors.ids().all(|i| anchors.get(i).unwrap().is_smooth()));

    let again = AnchorPath::from_bez(&anchors.to_bez());
    assert_eq!(again, anchors, "anchors -> path -> anchors is stable");
    let (a, b) = (original.bounding_box(), anchors.to_bez().bounding_box());
    assert!((a.x0 - b.x0).abs() < 1e-9 && (a.y1 - b.y1).abs() < 1e-9);
}

#[test]
fn quadratic_segments_become_equivalent_cubics() {
    let mut path = BezPath::new();
    path.move_to((0.0, 0.0));
    path.quad_to((30.0, 30.0), (60.0, 0.0));
    let before = path.bounding_box();
    let after = AnchorPath::from_bez(&path).to_bez().bounding_box();
    assert!(
        (before.y1 - after.y1).abs() < 1e-9,
        "{before:?} vs {after:?}"
    );
}

#[test]
fn splitting_a_segment_keeps_the_shape() {
    let original = Ellipse::new((0.0, 0.0), (30.0, 30.0), 0.0).to_path(0.1);
    let mut anchors = AnchorPath::from_bez(&original);
    let new = anchors.split_segment(id(0, 1), 0.5);
    assert_eq!(new, id(0, 2));
    assert_eq!(anchors.len(), 5);
    // The inserted point lies on the old curve.
    let p = anchors.get(new).unwrap().point;
    assert!(
        (p.to_vec2().hypot() - 30.0).abs() < 0.2,
        "{p:?} is off the circle"
    );

    // Splitting the closing segment appends rather than inserting at 0.
    let mut square = AnchorPath::from_bez(&Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1));
    let new = square.split_segment(id(0, 3), 0.5);
    assert_eq!(new, id(0, 4));
    assert!(close(square.get(new).unwrap().point, Point::new(0.0, 5.0)));
}

#[test]
fn removing_anchors_joins_neighbours_and_drops_degenerate_subpaths() {
    let mut anchors = AnchorPath::from_bez(&Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1));
    anchors.remove(&[id(0, 1)]);
    assert_eq!(anchors.len(), 3, "a triangle is left");
    anchors.remove(&[id(0, 0), id(0, 1)]);
    assert!(anchors.is_empty(), "one anchor is not a path");
}

#[test]
fn toggle_smooth_adds_handles_along_the_neighbours_then_removes_them() {
    let mut anchors = AnchorPath::from_bez(&Rect::new(0.0, 0.0, 30.0, 30.0).to_path(0.1));
    // Anchor 1 is (30, 0); its neighbours are (0, 0) and (30, 30).
    anchors.toggle_smooth(id(0, 1));
    let a = *anchors.get(id(0, 1)).unwrap();
    assert!(a.is_smooth());
    anchors.toggle_smooth(id(0, 1));
    let a = *anchors.get(id(0, 1)).unwrap();
    assert!(a.handle_in.is_none() && a.handle_out.is_none());
}

#[test]
fn hits_prefer_handles_then_anchors_then_segments() {
    let anchors = AnchorPath::from_bez(&Ellipse::new((0.0, 0.0), (30.0, 30.0), 0.0).to_path(0.1));
    let first = anchors.get(id(0, 0)).unwrap();
    let handle = first.handle_out.unwrap();

    assert_eq!(
        anchors.hit(first.point, 3.0, &[]),
        Some(PathHit::Anchor(id(0, 0)))
    );
    // Handles only count on anchors that show them.
    assert!(!matches!(
        anchors.hit(handle, 1.0, &[]),
        Some(PathHit::Handle(..))
    ));
    assert_eq!(
        anchors.hit(handle, 1.0, &[id(0, 0)]),
        Some(PathHit::Handle(id(0, 0), HandleSide::Out))
    );
    let on_curve = Point::new(30.0 * 0.5f64.sqrt(), 30.0 * 0.5f64.sqrt());
    assert!(matches!(
        anchors.hit(on_curve, 1.0, &[]),
        Some(PathHit::Segment { .. })
    ));
    assert_eq!(anchors.hit(Point::new(0.0, 0.0), 1.0, &[]), None);
}

// ---- Pen -----------------------------------------------------------------

#[test]
fn the_pen_draws_corners_and_smooth_points_as_one_undo_step() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let mut selection = Selection::new();

    let mut pen =
        PenSession::start(&mut doc, &mut selection, STROKE, Point::new(0.0, 0.0)).unwrap();
    assert!(!pen.release());
    // A plain click: a corner.
    pen.press(&mut doc, Point::new(50.0, 0.0), NONE, 4.0)
        .unwrap();
    assert!(!pen.release());
    // Press and drag: a smooth point, handles mirrored.
    pen.press(&mut doc, Point::new(50.0, 50.0), NONE, 4.0)
        .unwrap();
    pen.drag(&mut doc, Point::new(60.0, 50.0), NONE).unwrap();
    assert!(!pen.release());
    let smooth = pen.anchors()[2];
    assert!(close(smooth.handle_out.unwrap(), Point::new(60.0, 50.0)));
    assert!(close(smooth.handle_in.unwrap(), Point::new(40.0, 50.0)));

    let id = pen
        .finish(&mut doc, &mut journal, &mut selection)
        .unwrap()
        .unwrap();
    assert_eq!(selection.ids(), [id]);
    let anchors = anchors_of(&doc, id);
    assert_eq!(anchors.len(), 3);
    assert!(!anchors.subpaths[0].closed);
    // The stored path keeps the handle a segment uses; the dangling outgoing
    // handle of an open path's last anchor has nowhere to live (see anchors.rs).
    let stored = anchors.get(id_at(2)).unwrap();
    assert!(close(stored.handle_in.unwrap(), Point::new(40.0, 50.0)));

    // The whole path is one undo step.
    journal.undo(&mut doc).unwrap();
    assert!(!doc.is_attached(id));
    assert!(!journal.can_undo());
}

fn id_at(index: usize) -> AnchorId {
    id(0, index)
}

#[test]
fn pressing_the_first_anchor_closes_the_path() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let mut selection = Selection::new();
    let mut pen =
        PenSession::start(&mut doc, &mut selection, STROKE, Point::new(0.0, 0.0)).unwrap();
    pen.press(&mut doc, Point::new(40.0, 0.0), NONE, 4.0)
        .unwrap();
    pen.release();
    pen.press(&mut doc, Point::new(40.0, 40.0), NONE, 4.0)
        .unwrap();
    pen.release();

    pen.hover(Point::new(1.0, 1.0), 4.0);
    assert!(
        pen.closable(),
        "hovering the first anchor shows it will close"
    );
    pen.press(&mut doc, Point::new(1.0, 1.0), NONE, 4.0)
        .unwrap();
    assert!(pen.release(), "closing completes the path");
    let id = pen
        .finish(&mut doc, &mut journal, &mut selection)
        .unwrap()
        .unwrap();
    let anchors = anchors_of(&doc, id);
    assert!(anchors.subpaths[0].closed);
    assert_eq!(anchors.len(), 3, "closing adds no anchor");
}

#[test]
fn pressing_the_last_anchor_ends_the_path_open() {
    let mut doc = Document::new();
    let mut selection = Selection::new();
    let mut pen =
        PenSession::start(&mut doc, &mut selection, STROKE, Point::new(0.0, 0.0)).unwrap();
    // Pressing the only anchor again does nothing (no zero-length segment).
    pen.press(&mut doc, Point::new(0.5, 0.0), NONE, 4.0)
        .unwrap();
    assert!(!pen.release());
    assert_eq!(pen.anchors().len(), 1);

    pen.press(&mut doc, Point::new(40.0, 0.0), NONE, 4.0)
        .unwrap();
    pen.release();
    pen.press(&mut doc, Point::new(41.0, 0.0), NONE, 4.0)
        .unwrap();
    assert!(pen.release());
    assert_eq!(pen.anchors().len(), 2);
}

#[test]
fn shift_constrains_pen_anchors_to_45_degrees() {
    let mut doc = Document::new();
    let mut selection = Selection::new();
    let mut pen =
        PenSession::start(&mut doc, &mut selection, STROKE, Point::new(0.0, 0.0)).unwrap();
    pen.press(&mut doc, Point::new(40.0, 3.0), SHIFT, 4.0)
        .unwrap();
    let p = pen.anchors()[1].point;
    assert!(close(p, Point::new(40.0, 0.0)), "{p:?}");
}

#[test]
fn undo_while_drawing_removes_anchors_and_too_short_paths_vanish() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let mut selection = Selection::new();
    let mut pen =
        PenSession::start(&mut doc, &mut selection, STROKE, Point::new(0.0, 0.0)).unwrap();
    pen.press(&mut doc, Point::new(40.0, 0.0), NONE, 4.0)
        .unwrap();
    pen.release();
    assert!(pen.undo_anchor(&mut doc).unwrap());
    assert_eq!(pen.anchors().len(), 1);

    let node = pen.id();
    assert_eq!(
        pen.finish(&mut doc, &mut journal, &mut selection).unwrap(),
        None
    );
    assert!(!doc.is_attached(node));
    assert!(!journal.can_undo());
}

// ---- Path editing --------------------------------------------------------

fn edit_square(doc: &mut Document, journal: &mut Journal) -> (NodeId, PathEdit) {
    let node = insert(doc, journal, Rect::new(0.0, 0.0, 40.0, 40.0).to_path(0.1));
    let edit = PathEdit::begin(doc, node).unwrap().unwrap();
    (node, edit)
}

#[test]
fn dragging_an_anchor_is_one_undo_step() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let (node, mut edit) = edit_square(&mut doc, &mut journal);

    assert_eq!(
        edit.press(&mut doc, Point::new(40.0, 40.0), 4.0, false)
            .unwrap(),
        PressOutcome::Anchor
    );
    for step in 1..=10 {
        let p = Point::new(40.0 + step as f64, 40.0 + step as f64);
        edit.update(&mut doc, p, NONE).unwrap();
    }
    assert!(edit.release(&mut doc, &mut journal).unwrap());
    assert!(close(
        anchors_of(&doc, node).get(id(0, 2)).unwrap().point,
        Point::new(50.0, 50.0)
    ));

    journal.undo(&mut doc).unwrap();
    assert!(close(
        anchors_of(&doc, node).get(id(0, 2)).unwrap().point,
        Point::new(40.0, 40.0)
    ));
    journal.undo(&mut doc).unwrap();
    assert!(!doc.is_attached(node), "the next undo is the insert");
}

#[test]
fn a_press_without_movement_records_nothing() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let (_, mut edit) = edit_square(&mut doc, &mut journal);
    edit.press(&mut doc, Point::new(0.0, 0.0), 4.0, false)
        .unwrap();
    assert!(!edit.release(&mut doc, &mut journal).unwrap());
    journal.undo(&mut doc).unwrap();
    assert!(!journal.can_undo(), "only the insert was journaled");
}

#[test]
fn clicking_a_segment_inserts_an_anchor_in_one_step() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let (node, mut edit) = edit_square(&mut doc, &mut journal);

    assert_eq!(
        edit.press(&mut doc, Point::new(20.0, 0.5), 4.0, false)
            .unwrap(),
        PressOutcome::Segment
    );
    assert!(edit.release(&mut doc, &mut journal).unwrap());
    assert_eq!(anchors_of(&doc, node).len(), 5);
    assert_eq!(edit.selected(), [id(0, 1)]);
    journal.undo(&mut doc).unwrap();
    assert_eq!(anchors_of(&doc, node).len(), 4);
}

#[test]
fn dragging_a_smooth_handle_swings_the_other_unless_alt() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let node = insert(
        &mut doc,
        &mut journal,
        Ellipse::new((0.0, 0.0), (30.0, 30.0), 0.0).to_path(0.1),
    );
    let mut edit = PathEdit::begin(&doc, node).unwrap().unwrap();
    let anchor = anchors_of(&doc, node).get(id(0, 0)).copied().unwrap();

    // Select the anchor so its handles are live, then grab the out handle.
    edit.press(&mut doc, anchor.point, 2.0, false).unwrap();
    edit.release(&mut doc, &mut journal).unwrap();
    let handle = anchor.handle_out.unwrap();
    assert_eq!(
        edit.press(&mut doc, handle, 2.0, false).unwrap(),
        PressOutcome::Handle
    );
    edit.update(&mut doc, handle + (10.0, 0.0), NONE).unwrap();
    edit.release(&mut doc, &mut journal).unwrap();
    let a = anchors_of(&doc, node).get(id(0, 0)).copied().unwrap();
    assert!(a.is_smooth(), "the opposite handle followed");
    let reach_in = (anchor.handle_in.unwrap() - anchor.point).hypot();
    assert!(((a.handle_in.unwrap() - a.point).hypot() - reach_in).abs() < 1e-9);

    let handle = a.handle_out.unwrap();
    edit.press(&mut doc, handle, 2.0, false).unwrap();
    edit.update(&mut doc, handle + (10.0, 0.0), ALT).unwrap();
    edit.release(&mut doc, &mut journal).unwrap();
    let broken = anchors_of(&doc, node).get(id(0, 0)).copied().unwrap();
    assert!(!broken.is_smooth(), "Alt breaks the pair");
    assert_eq!(broken.handle_in, a.handle_in);
}

#[test]
fn editing_follows_the_node_transform() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let node = insert(
        &mut doc,
        &mut journal,
        Rect::new(0.0, 0.0, 40.0, 40.0).to_path(0.1),
    );
    journal
        .execute(
            &mut doc,
            Command::SetTransform {
                id: node,
                transform: Affine::translate((100.0, 0.0)) * Affine::scale(2.0),
            },
        )
        .unwrap();
    let mut edit = PathEdit::begin(&doc, node).unwrap().unwrap();

    // Local (40, 0) is at document (180, 0).
    assert_eq!(
        edit.press(&mut doc, Point::new(180.0, 0.0), 4.0, false)
            .unwrap(),
        PressOutcome::Anchor
    );
    edit.update(&mut doc, Point::new(200.0, 0.0), NONE).unwrap();
    edit.release(&mut doc, &mut journal).unwrap();
    // 20 document units is 10 local ones at scale 2.
    assert!(close(
        anchors_of(&doc, node).get(id(0, 1)).unwrap().point,
        Point::new(50.0, 0.0)
    ));

    let view = edit.view(&doc).unwrap();
    assert!(
        close(view.anchors[1].0, Point::new(200.0, 0.0)),
        "the overlay is in document space"
    );
}

#[test]
fn deleting_anchors_and_emptying_the_path() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let (node, mut edit) = edit_square(&mut doc, &mut journal);

    assert_eq!(
        edit.delete_selected(&mut doc, &mut journal).unwrap(),
        DeleteOutcome::Nothing
    );
    edit.press(&mut doc, Point::new(0.0, 0.0), 4.0, false)
        .unwrap();
    edit.release(&mut doc, &mut journal).unwrap();
    assert_eq!(
        edit.delete_selected(&mut doc, &mut journal).unwrap(),
        DeleteOutcome::Anchors
    );
    assert_eq!(anchors_of(&doc, node).len(), 3);

    for p in [(40.0, 0.0), (40.0, 40.0), (0.0, 40.0)] {
        edit.press(&mut doc, p.into(), 4.0, true).unwrap();
        edit.release(&mut doc, &mut journal).unwrap();
    }
    assert_eq!(edit.selected().len(), 3);
    assert_eq!(
        edit.delete_selected(&mut doc, &mut journal).unwrap(),
        DeleteOutcome::EmptiedPath
    );
}

#[test]
fn double_click_toggles_corner_and_smooth() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let (node, mut edit) = edit_square(&mut doc, &mut journal);
    assert!(
        edit.toggle_smooth_at(&mut doc, &mut journal, Point::new(40.0, 0.0), 4.0)
            .unwrap()
    );
    assert!(anchors_of(&doc, node).get(id(0, 1)).unwrap().is_smooth());
    assert!(
        !edit
            .toggle_smooth_at(&mut doc, &mut journal, Point::new(20.0, 20.0), 4.0)
            .unwrap()
    );
    journal.undo(&mut doc).unwrap();
    assert!(!anchors_of(&doc, node).get(id(0, 1)).unwrap().is_smooth());
}

#[test]
fn cancelling_an_edit_drag_restores_the_path() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let (node, mut edit) = edit_square(&mut doc, &mut journal);
    let before = doc.vector_path(node).unwrap().clone();
    edit.press(&mut doc, Point::new(20.0, 0.0), 4.0, false)
        .unwrap();
    edit.update(&mut doc, Point::new(20.0, -30.0), NONE)
        .unwrap();
    edit.cancel_drag(&mut doc).unwrap();
    assert_eq!(doc.vector_path(node).unwrap(), &before);
}
