//! Paint as a whole: the defaults (D) and swapping fill and stroke
//! (Shift+X), each one undo step over the selection. The eyedropper, which
//! shares the module, is a tool and is tested with the others.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::geom::{Rect, Shape};
use graphicgene_core::node::{Dash, Node, NodeId, NodeKind, Stroke};
use graphicgene_core::paint::{Gradient, GradientKind, Paint};
use graphicgene_core::session::Session;
use graphicgene_core::text::{TextNode, TextStyle};

const RED: LinearRgba = LinearRgba::new(1.0, 0.0, 0.0, 1.0);
const BLUE: LinearRgba = LinearRgba::new(0.0, 0.0, 1.0, 1.0);

fn shape(session: &mut Session, fill: Option<Paint>, stroke: Option<Stroke>) -> NodeId {
    let mut node = Node::vector("Shape", Rect::new(0.0, 0.0, 20.0, 20.0).to_path(0.1), None);
    if let NodeKind::Vector(v) = &mut node.kind {
        v.fill = fill;
        v.stroke = stroke;
    }
    session.insert(node).unwrap()
}

fn text(session: &mut Session) -> NodeId {
    let node = Node::text("Text", TextNode::new("Hi", TextStyle::default(), Some(RED)));
    session.insert(node).unwrap()
}

fn paint(session: &Session, id: NodeId) -> (Option<Paint>, Option<Stroke>) {
    match &session.document().get(id).unwrap().kind {
        NodeKind::Vector(v) => (v.fill.clone(), v.stroke),
        NodeKind::Text(t) => (t.fill.clone(), None),
        NodeKind::Group(_) => (None, None),
    }
}

fn select(session: &mut Session, ids: &[NodeId]) {
    session.clear_selection().unwrap();
    for &id in ids {
        session.select_layer(id, true).unwrap();
    }
}

/// Red, stroked blue three units wide and dashed.
fn styled() -> Stroke {
    Stroke {
        dash: Some(Dash {
            length: 4.0,
            gap: 2.0,
        }),
        ..Stroke::solid(BLUE, 3.0)
    }
}

#[test]
fn d_paints_shapes_white_stroked_black_and_text_black() {
    let mut session = Session::new();
    let a = shape(&mut session, Some(Paint::Solid(RED)), Some(styled()));
    let t = text(&mut session);
    select(&mut session, &[a, t]);
    assert!(session.default_paint().unwrap());
    assert_eq!(
        paint(&session, a),
        (
            Some(Paint::Solid(LinearRgba::WHITE)),
            Some(Stroke::solid(LinearRgba::BLACK, 1.0))
        )
    );
    assert_eq!(paint(&session, t).0, Some(Paint::Solid(LinearRgba::BLACK)));
    assert!(!session.default_paint().unwrap(), "again changes nothing");

    session.undo().unwrap();
    assert_eq!(
        paint(&session, a),
        (Some(Paint::Solid(RED)), Some(styled()))
    );
    assert_eq!(
        paint(&session, t).0,
        Some(Paint::Solid(RED)),
        "one undo step"
    );
}

#[test]
fn shift_x_swaps_the_colours_and_keeps_the_strokes_style() {
    let mut session = Session::new();
    let both = shape(&mut session, Some(Paint::Solid(RED)), Some(styled()));
    let fill_only = shape(&mut session, Some(Paint::Solid(RED)), None);
    let t = text(&mut session);
    select(&mut session, &[both, fill_only, t]);
    assert!(session.swap_paint().unwrap());

    let (fill, stroke) = paint(&session, both);
    assert_eq!(fill, Some(Paint::Solid(BLUE)));
    assert_eq!(
        stroke,
        Some(Stroke {
            color: RED,
            ..styled()
        }),
        "width and dashes kept"
    );
    assert_eq!(
        paint(&session, fill_only),
        (None, Some(Stroke::solid(RED, 1.0))),
        "a stroke where there was none gets the default width"
    );
    assert_eq!(
        paint(&session, t).0,
        Some(Paint::Solid(RED)),
        "text has no stroke to swap with"
    );

    assert!(session.swap_paint().unwrap());
    assert_eq!(
        paint(&session, both),
        (Some(Paint::Solid(RED)), Some(styled()))
    );
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(paint(&session, fill_only), (Some(Paint::Solid(RED)), None));
}

#[test]
fn a_gradient_swapped_into_a_stroke_is_its_first_colour() {
    let mut session = Session::new();
    let gradient = Gradient::fading(GradientKind::Linear, BLUE);
    let id = shape(&mut session, Some(Paint::Gradient(gradient)), None);
    select(&mut session, &[id]);
    session.swap_paint().unwrap();
    assert_eq!(paint(&session, id), (None, Some(Stroke::solid(BLUE, 1.0))));
}

#[test]
fn nothing_to_swap_is_nothing_to_undo() {
    let mut session = Session::new();
    let bare = shape(&mut session, None, None);
    select(&mut session, &[bare]);
    assert!(!session.swap_paint().unwrap());
    session.clear_selection().unwrap();
    assert!(!session.default_paint().unwrap(), "nothing selected");
}
