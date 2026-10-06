//! Stroke styles reach the pixels: dashes leave gaps, and caps decide
//! whether ink reaches past a line's ends.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{BezPath, Size};
use graphicgene_core::node::{Dash, LineCap, Node, NodeKind, Stroke};
use graphicgene_render::export::{Image, image};

/// A 4-wide line from (10, 10) to (90, 10) on a 100 × 20 artboard.
fn line(stroke: Stroke) -> Image {
    let mut doc = Document::with_artboard(Size::new(100.0, 20.0));
    let root = doc.root();
    let mut path = BezPath::new();
    path.move_to((10.0, 10.0));
    path.line_to((90.0, 10.0));
    let mut node = Node::vector("Line", path, None);
    if let NodeKind::Vector(v) = &mut node.kind {
        v.stroke = Some(stroke);
    }
    let id = doc.insert_detached(node);
    doc.attach(id, root, 0).unwrap();
    image(&doc, 1.0, true).unwrap()
}

/// Coverage of the pixel whose top-left corner is (x, y).
fn alpha(image: &Image, x: u32, y: u32) -> u8 {
    image.pixels[((y * image.width + x) * 4 + 3) as usize]
}

#[test]
fn dashes_leave_gaps() {
    let solid = line(Stroke::solid(LinearRgba::BLACK, 4.0));
    let dashed = line(Stroke {
        dash: Some(Dash {
            length: 10.0,
            gap: 10.0,
        }),
        ..Stroke::solid(LinearRgba::BLACK, 4.0)
    });
    // From x = 10: ink to 20, none to 30, ink to 40.
    for x in [14, 24, 34] {
        assert_eq!(alpha(&solid, x, 9), 255, "solid at {x}");
    }
    assert_eq!(alpha(&dashed, 14, 9), 255);
    assert_eq!(alpha(&dashed, 24, 9), 0, "a gap");
    assert_eq!(alpha(&dashed, 34, 9), 255);
}

#[test]
fn round_and_square_caps_reach_past_the_ends() {
    let cap = |cap| {
        line(Stroke {
            cap,
            ..Stroke::solid(LinearRgba::BLACK, 4.0)
        })
    };
    // The pixel from x = 8 to 9, just before the line starts at 10.
    assert_eq!(alpha(&cap(LineCap::Butt), 8, 9), 0);
    assert!(alpha(&cap(LineCap::Round), 8, 9) > 0);
    assert_eq!(alpha(&cap(LineCap::Square), 8, 9), 255);
}
