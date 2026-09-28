//! The properties panel: what it reads from the selection, and edits made
//! through it — one undo step per edit however many values it previewed.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::command::Command;
use graphicgene_core::geom::{Affine, Point, Rect, Shape, Vec2};
use graphicgene_core::gesture::{Frame, TransformKind};
use graphicgene_core::node::{Node, NodeId, Stroke};
use graphicgene_core::properties::{Properties, Property, Shared};
use graphicgene_core::session::Session;

const RED: LinearRgba = LinearRgba::new(1.0, 0.0, 0.0, 1.0);
const EPS: f64 = 1e-9;

fn rect(session: &mut Session, x: f64, y: f64, w: f64, h: f64) -> NodeId {
    let path = Rect::new(x, y, x + w, y + h).to_path(0.1);
    session
        .insert(Node::vector("Rectangle", path, Some(LinearRgba::BLACK)))
        .unwrap()
}

fn props(session: &Session) -> Properties {
    session
        .properties()
        .unwrap()
        .expect("something is selected")
}

fn select(session: &mut Session, ids: &[NodeId]) {
    session.clear_selection().unwrap();
    for &id in ids {
        session.select_layer(id, true).unwrap();
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn assert_frame(p: &Properties, x: f64, y: f64, w: f64, h: f64, rotation: f64) {
    let got = (p.x, p.y, p.width, p.height, p.rotation);
    assert!(
        close(p.x, x)
            && close(p.y, y)
            && close(p.width, w)
            && close(p.height, h)
            && close(p.rotation, rotation),
        "got {got:?}, want {:?}",
        (x, y, w, h, rotation)
    );
}

fn centre(session: &Session) -> Point {
    let frame = Frame::of(session.document(), session.selection().ids())
        .unwrap()
        .unwrap();
    frame.point_at(0.5, 0.5)
}

#[test]
fn the_panel_reads_what_the_selection_shares() {
    let mut session = Session::new();
    let a = rect(&mut session, 10.0, 20.0, 100.0, 50.0);
    let b = rect(&mut session, 200.0, 100.0, 40.0, 40.0);
    assert_eq!(session.properties().unwrap(), None, "nothing selected");

    select(&mut session, &[a]);
    let p = props(&session);
    assert_eq!(p.count, 1);
    assert_frame(&p, 10.0, 20.0, 100.0, 50.0, 0.0);
    assert_eq!(p.opacity, Shared::Same(1.0));
    assert_eq!(p.fill, Some(Shared::Same(Some(LinearRgba::BLACK))));
    assert_eq!(p.stroke, Some(Shared::Same(None)));
    assert_eq!(p.stroke_width, None, "nothing is stroked");

    // Several nodes share the box around them, unrotated.
    session.set_property(Property::Fill(Some(RED))).unwrap();
    select(&mut session, &[a, b]);
    let p = props(&session);
    assert_eq!(p.count, 2);
    assert_frame(&p, 10.0, 20.0, 230.0, 120.0, 0.0);
    assert_eq!(p.fill, Some(Shared::Mixed));
    assert_eq!(p.opacity, Shared::Same(1.0));
}

#[test]
fn a_scrub_through_many_values_is_one_undo_step() {
    let mut session = Session::new();
    let a = rect(&mut session, 10.0, 20.0, 100.0, 50.0);
    select(&mut session, &[a]);

    for x in [11.0, 15.0, 40.0, 80.0] {
        assert!(session.preview_property(Property::X(x)).unwrap());
        assert!(close(props(&session).x, x), "the preview shows");
        assert!(session.busy(), "a preview is not saved");
    }
    assert!(session.commit_property().unwrap());
    assert!(!session.busy());
    assert_frame(&props(&session), 80.0, 20.0, 100.0, 50.0, 0.0);

    // One undo takes it back to before the scrub, not to the last preview.
    assert!(session.undo().unwrap());
    assert_frame(&props(&session), 10.0, 20.0, 100.0, 50.0, 0.0);
    assert!(session.undo().unwrap(), "the insert");
    assert!(!session.can_undo());
    session.redo().unwrap();
    session.select_layer(a, false).unwrap();
    session.redo().unwrap();
    assert_frame(&props(&session), 80.0, 20.0, 100.0, 50.0, 0.0);
}

#[test]
fn a_value_dragged_back_to_its_start_records_nothing() {
    let mut session = Session::new();
    let a = rect(&mut session, 10.0, 20.0, 100.0, 50.0);
    rect(&mut session, 200.0, 20.0, 10.0, 10.0);
    session.undo().unwrap();
    assert!(session.can_redo());
    select(&mut session, &[a]);

    session.preview_property(Property::Opacity(0.2)).unwrap();
    session.preview_property(Property::Opacity(1.0)).unwrap();
    assert!(!session.commit_property().unwrap());
    assert!(session.can_redo(), "recording nothing kept the redo stack");

    // Retyping the value it already has is no step either, and nonsense
    // from a text field is ignored.
    assert!(!session.set_property(Property::Y(20.0)).unwrap());
    assert!(!session.set_property(Property::Width(f64::NAN)).unwrap());
    assert!(
        !session
            .set_property(Property::Rotation(f64::INFINITY))
            .unwrap()
    );
    assert!(session.can_redo());
}

#[test]
fn cancel_puts_back_the_values_from_before_the_edit() {
    let mut session = Session::new();
    let a = rect(&mut session, 0.0, 0.0, 100.0, 50.0);
    select(&mut session, &[a]);
    session.preview_property(Property::Opacity(0.25)).unwrap();
    session.preview_property(Property::Opacity(0.5)).unwrap();
    assert_eq!(props(&session).opacity, Shared::Same(0.5));

    assert!(session.cancel_property().unwrap());
    assert_eq!(props(&session).opacity, Shared::Same(1.0));
    assert!(!session.busy());
    assert!(
        !session.cancel_property().unwrap(),
        "nothing left to cancel"
    );
    session.undo().unwrap();
    assert!(!session.can_undo(), "only the insert was recorded");
}

#[test]
fn size_keeps_the_top_left_corner_and_rotation_the_centre() {
    let mut session = Session::new();
    let a = rect(&mut session, 0.0, 0.0, 100.0, 50.0);
    select(&mut session, &[a]);

    session.set_property(Property::Width(200.0)).unwrap();
    assert_frame(&props(&session), 0.0, 0.0, 200.0, 50.0, 0.0);
    session.set_property(Property::Height(10.0)).unwrap();
    session.set_property(Property::Height(-5.0)).unwrap();
    let p = props(&session);
    assert!(p.height > 0.0 && p.height < 0.1, "clamped, never flipped");
    session.set_property(Property::Height(50.0)).unwrap();

    // Counter-clockwise, as the panel reads it: the top edge now points up
    // the screen, and the old top-left corner has swung below the centre.
    let before = centre(&session);
    session.set_property(Property::Rotation(90.0)).unwrap();
    assert!((centre(&session) - before).hypot() < EPS);
    assert_frame(&props(&session), 75.0, 125.0, 200.0, 50.0, 90.0);

    // A rotated node resizes along its own edges, still from its corner.
    session.set_property(Property::Width(100.0)).unwrap();
    assert_frame(&props(&session), 75.0, 125.0, 100.0, 50.0, 90.0);
    session.set_property(Property::X(0.0)).unwrap();
    assert_frame(&props(&session), 0.0, 125.0, 100.0, 50.0, 90.0);

    // Angles wrap into (-180, 180].
    session.set_property(Property::Rotation(270.0)).unwrap();
    assert!(close(props(&session).rotation, -90.0));
    session.set_property(Property::Rotation(-180.0)).unwrap();
    assert!(close(props(&session).rotation, 180.0));
}

#[test]
fn several_nodes_move_scale_and_rotate_as_one_box() {
    let mut session = Session::new();
    let a = rect(&mut session, 0.0, 0.0, 100.0, 100.0);
    let b = rect(&mut session, 200.0, 100.0, 100.0, 100.0);
    select(&mut session, &[a, b]);

    session.set_property(Property::Width(600.0)).unwrap();
    assert_frame(&props(&session), 0.0, 0.0, 600.0, 200.0, 0.0);
    select(&mut session, &[b]);
    assert_frame(&props(&session), 400.0, 100.0, 200.0, 100.0, 0.0);

    // The box stays unrotated, so its rotation always reads 0; a value
    // entered turns everything about the box's centre.
    select(&mut session, &[a, b]);
    let before = centre(&session);
    session.set_property(Property::Rotation(180.0)).unwrap();
    assert!((centre(&session) - before).hypot() < 1e-6);
    let p = props(&session);
    assert!(close(p.rotation, 0.0));
    select(&mut session, &[a]);
    assert_frame(&props(&session), 600.0, 200.0, 200.0, 100.0, 180.0);
}

#[test]
fn position_is_in_document_space_whatever_the_parent() {
    let mut session = Session::new();
    let root = session.document().root();
    session
        .execute(Command::InsertNode {
            parent: root,
            index: 0,
            node: Node::group("Group"),
        })
        .unwrap();
    let group = session.document().children_of(root).unwrap()[0];
    let mut child = Node::vector(
        "Child",
        Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1),
        Some(RED),
    );
    child.common.transform = Affine::translate(Vec2::new(5.0, 5.0));
    session
        .execute(Command::InsertNode {
            parent: group,
            index: 0,
            node: child,
        })
        .unwrap();
    let child = session.document().children_of(group).unwrap()[0];
    let transform = Affine::translate(Vec2::new(100.0, 0.0)) * Affine::scale(2.0);
    session
        .execute(Command::SetTransform {
            id: group,
            transform,
        })
        .unwrap();

    select(&mut session, &[child]);
    assert_frame(&props(&session), 110.0, 10.0, 20.0, 20.0, 0.0);
    session.set_property(Property::X(0.0)).unwrap();
    session.set_property(Property::Width(40.0)).unwrap();
    assert_frame(&props(&session), 0.0, 10.0, 40.0, 20.0, 0.0);

    // Retyping a value is no step, even where the parent's transform does
    // not invert exactly.
    let tilted = Affine::rotate(0.3) * Affine::scale(1.7);
    session
        .execute(Command::SetTransform {
            id: group,
            transform: tilted,
        })
        .unwrap();
    select(&mut session, &[child]);
    let p = props(&session);
    for property in [
        Property::X(p.x),
        Property::Y(p.y),
        Property::Width(p.width),
        Property::Height(p.height),
        Property::Rotation(p.rotation),
        Property::Rotation(p.rotation + 360.0),
    ] {
        assert!(
            !session.set_property(property.clone()).unwrap(),
            "{property:?}"
        );
    }

    // A group has no paint of its own.
    select(&mut session, &[group]);
    let p = props(&session);
    assert_eq!((p.fill, p.stroke, p.stroke_width), (None, None, None));
    assert!(!session.set_property(Property::Fill(None)).unwrap());
}

#[test]
fn stroke_colour_and_width_change_separately() {
    let mut session = Session::new();
    let a = rect(&mut session, 0.0, 0.0, 10.0, 10.0);
    let b = rect(&mut session, 20.0, 0.0, 10.0, 10.0);
    select(&mut session, &[a]);
    let stroke = Stroke {
        color: LinearRgba::BLACK,
        width: 2.0,
    };
    session
        .set_property(Property::Stroke(Some(stroke)))
        .unwrap();

    select(&mut session, &[a, b]);
    let p = props(&session);
    assert_eq!(p.stroke, Some(Shared::Mixed), "one stroked, one not");
    assert_eq!(p.stroke_width, Some(Shared::Same(2.0)), "of those stroked");

    // Width leaves the unstroked alone...
    session.set_property(Property::StrokeWidth(4.0)).unwrap();
    let p = props(&session);
    assert_eq!(p.stroke, Some(Shared::Mixed));
    assert_eq!(p.stroke_width, Some(Shared::Same(4.0)));

    // ...a colour strokes everything, keeping widths that exist.
    session.set_property(Property::StrokeColor(RED)).unwrap();
    let p = props(&session);
    assert_eq!(p.stroke, Some(Shared::Same(Some(RED))));
    assert_eq!(p.stroke_width, Some(Shared::Mixed));

    session.set_property(Property::Stroke(None)).unwrap();
    let p = props(&session);
    assert_eq!(p.stroke, Some(Shared::Same(None)));
    assert_eq!(p.stroke_width, None);

    // Each was one step.
    for _ in 0..3 {
        session.undo().unwrap();
    }
    select(&mut session, &[a, b]);
    let p = props(&session);
    assert_eq!(p.stroke, Some(Shared::Mixed));
    assert_eq!(p.stroke_width, Some(Shared::Same(2.0)));
}

#[test]
fn fill_and_opacity() {
    let mut session = Session::new();
    let a = rect(&mut session, 0.0, 0.0, 10.0, 10.0);
    select(&mut session, &[a]);
    session.set_property(Property::Fill(None)).unwrap();
    assert_eq!(props(&session).fill, Some(Shared::Same(None)));
    session.set_property(Property::Opacity(2.0)).unwrap();
    assert_eq!(props(&session).opacity, Shared::Same(1.0), "clamped");
    session.set_property(Property::Opacity(0.5)).unwrap();
    assert_eq!(props(&session).opacity, Shared::Same(0.5));
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(
        props(&session).fill,
        Some(Shared::Same(Some(LinearRgba::BLACK)))
    );
}

#[test]
fn anything_else_abandons_an_open_preview() {
    let mut session = Session::new();
    let a = rect(&mut session, 0.0, 0.0, 10.0, 10.0);
    let b = rect(&mut session, 50.0, 0.0, 10.0, 10.0);
    select(&mut session, &[a]);

    // Undo drops the preview first, then undoes the step before it.
    session.preview_property(Property::X(30.0)).unwrap();
    session.undo().unwrap();
    assert!(!session.busy());
    assert!(!session.document().is_attached(b));
    assert_frame(&props(&session), 0.0, 0.0, 10.0, 10.0, 0.0);

    // So does a new selection, and a press on the canvas.
    session.preview_property(Property::X(30.0)).unwrap();
    session.select_all().unwrap();
    assert!(!session.busy());
    select(&mut session, &[a]);
    assert_frame(&props(&session), 0.0, 0.0, 10.0, 10.0, 0.0);
    session.preview_property(Property::X(30.0)).unwrap();
    assert!(
        session
            .begin_transform(TransformKind::Move, Point::new(5.0, 5.0))
            .unwrap()
    );
    session.cancel_gesture().unwrap();
    assert_frame(&props(&session), 0.0, 0.0, 10.0, 10.0, 0.0);

    // And the other way round: no preview during a drag or the pen.
    session
        .begin_transform(TransformKind::Move, Point::new(5.0, 5.0))
        .unwrap();
    assert!(!session.preview_property(Property::X(30.0)).unwrap());
    session.cancel_gesture().unwrap();
    let stroke = Stroke {
        color: LinearRgba::BLACK,
        width: 1.0,
    };
    session
        .pen_press(Point::new(100.0, 100.0), false, 4.0, stroke)
        .unwrap();
    session.pen_release().unwrap();
    assert_eq!(session.properties().unwrap(), None);
    assert!(!session.set_property(Property::X(0.0)).unwrap());
}
