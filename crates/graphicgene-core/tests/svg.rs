//! SVG export.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::command::{Command, Journal};
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Affine, BezPath, Rect, Shape, Size};
use graphicgene_core::node::{BlendMode, Dash, LineCap, LineJoin, Node, NodeId, NodeKind, Stroke};
use graphicgene_core::svg::to_svg;

fn insert(doc: &mut Document, journal: &mut Journal, parent: NodeId, node: Node) -> NodeId {
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

fn square() -> BezPath {
    Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1)
}

#[test]
fn an_empty_document_is_an_empty_svg_the_size_of_its_artboard() {
    let svg = to_svg(&Document::with_artboard(Size::new(800.0, 600.0))).unwrap();
    assert_eq!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"800\" height=\"600\" viewBox=\"0 0 800 600\">\n</svg>\n"
    );
}

#[test]
fn fills_are_srgb_hex_in_paint_order() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let root = doc.root();
    let red = LinearRgba::from_srgb8(255, 0, 0, 255);
    let blue = LinearRgba::from_srgb8(0, 0, 255, 128);
    insert(
        &mut doc,
        &mut journal,
        root,
        Node::vector("Below", square(), Some(red)),
    );
    insert(
        &mut doc,
        &mut journal,
        root,
        Node::vector("Above", square(), Some(blue)),
    );

    let svg = to_svg(&doc).unwrap();
    let below = svg.find(r#"data-name="Below""#).unwrap();
    let above = svg.find(r#"data-name="Above""#).unwrap();
    assert!(
        below < above,
        "later siblings paint on top, so they come later"
    );
    assert!(svg.contains(r##"fill="#ff0000""##), "{svg}");
    assert!(
        svg.contains(r##"fill="#0000ff" fill-opacity="0.502""##),
        "{svg}"
    );
    assert!(svg.contains(r#"d="M0,0 L10,0 L10,10 L0,10 Z""#), "{svg}");
}

#[test]
fn transforms_opacity_and_blend_are_attributes_not_baked_geometry() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let root = doc.root();
    let mut node = Node::vector("Moved", square(), Some(LinearRgba::BLACK));
    node.common.transform = Affine::translate((20.0, 30.0)) * Affine::scale(2.0);
    node.common.opacity = 0.25;
    node.common.blend_mode = BlendMode::Multiply;
    insert(&mut doc, &mut journal, root, node);

    let svg = to_svg(&doc).unwrap();
    assert!(
        svg.contains(r#"transform="matrix(2 0 0 2 20 30)""#),
        "{svg}"
    );
    assert!(svg.contains(r#"opacity="0.25""#), "{svg}");
    assert!(svg.contains(r#"style="mix-blend-mode:multiply""#), "{svg}");
    // The path itself is untouched.
    assert!(svg.contains(r#"d="M0,0 L10,0"#), "{svg}");
}

#[test]
fn groups_nest_hidden_nodes_are_omitted_and_strokes_are_written() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let root = doc.root();
    let group = insert(&mut doc, &mut journal, root, Node::group("Group"));
    let mut stroked = Node::vector("Line", square(), None);
    if let NodeKind::Vector(v) = &mut stroked.kind {
        v.stroke = Some(Stroke::solid(LinearRgba::BLACK, 2.0));
    }
    insert(&mut doc, &mut journal, group, stroked);
    let mut hidden = Node::vector("Hidden", square(), Some(LinearRgba::BLACK));
    hidden.common.visible = false;
    insert(&mut doc, &mut journal, group, hidden);

    let svg = to_svg(&doc).unwrap();
    assert!(
        svg.contains("<g data-name=\"Group\">\n    <path data-name=\"Line\""),
        "{svg}"
    );
    assert!(
        svg.contains(r##"fill="none" stroke="#000000" stroke-width="2""##),
        "{svg}"
    );
    assert!(!svg.contains("Hidden"), "{svg}");
}

#[test]
fn names_are_escaped() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let root = doc.root();
    let name = r#"<a & "b">"#;
    insert(
        &mut doc,
        &mut journal,
        root,
        Node::vector(name, square(), None),
    );
    let svg = to_svg(&doc).unwrap();
    assert!(
        svg.contains(r#"data-name="&lt;a &amp; &quot;b&quot;&gt;""#),
        "{svg}"
    );
}

#[test]
fn stroke_styles_are_written_when_not_svg_defaults() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let root = doc.root();
    for (name, cap, join, dash) in [
        ("Plain", LineCap::Butt, LineJoin::Miter, None),
        (
            "Styled",
            LineCap::Round,
            LineJoin::Bevel,
            Some(Dash {
                length: 4.0,
                gap: 1.5,
            }),
        ),
    ] {
        let mut node = Node::vector(name, square(), None);
        if let NodeKind::Vector(v) = &mut node.kind {
            v.stroke = Some(Stroke {
                cap,
                join,
                dash,
                ..Stroke::solid(LinearRgba::BLACK, 2.0)
            });
        }
        insert(&mut doc, &mut journal, root, node);
    }
    let svg = to_svg(&doc).unwrap();
    let line = |name: &str| svg.lines().find(|l| l.contains(name)).unwrap().to_owned();
    assert!(
        !line("Plain").contains("linecap") && !line("Plain").contains("dash"),
        "{svg}"
    );
    assert!(
        line("Styled")
            .contains(r#"stroke-linecap="round" stroke-linejoin="bevel" stroke-dasharray="4 1.5""#),
        "{svg}"
    );
}
