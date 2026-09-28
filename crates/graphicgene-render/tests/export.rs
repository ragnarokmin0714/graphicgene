//! Raster export: the artboard at a chosen scale, as straight-alpha pixels.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Rect, Shape, Size};
use graphicgene_core::node::Node;
use graphicgene_render::RenderError;
use graphicgene_render::export::{Image, image};

const RED: LinearRgba = LinearRgba::new(1.0, 0.0, 0.0, 1.0);

/// An 80 × 60 artboard with a red square from (10, 10) to (30, 30), and a
/// half-transparent one from (40, 10) to (60, 30).
fn document() -> Document {
    let mut doc = Document::with_artboard(Size::new(80.0, 60.0));
    let root = doc.root();
    for (x, alpha) in [(10.0, 1.0), (40.0, 0.5)] {
        let square = Rect::new(x, 10.0, x + 20.0, 30.0).to_path(0.1);
        let fill = LinearRgba { a: alpha, ..RED };
        let id = doc.insert_detached(Node::vector("Square", square, Some(fill)));
        let index = doc.children_of(root).unwrap().len();
        doc.attach(id, root, index).unwrap();
    }
    doc
}

fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * image.width + x) * 4) as usize;
    image.pixels[at..at + 4].try_into().unwrap()
}

#[test]
fn an_export_is_the_artboard_at_its_scale() {
    let doc = document();
    let one = image(&doc, 1.0, false).unwrap();
    assert_eq!(
        (one.width, one.height, one.pixels.len()),
        (80, 60, 80 * 60 * 4)
    );
    assert_eq!(pixel(&one, 20, 20), [255, 0, 0, 255]);
    assert_eq!(
        pixel(&one, 70, 50),
        [255, 255, 255, 255],
        "the page is white"
    );

    let two = image(&doc, 2.0, false).unwrap();
    assert_eq!((two.width, two.height), (160, 120));
    assert_eq!(pixel(&two, 40, 40), [255, 0, 0, 255]);
    assert_eq!(
        pixel(&two, 61, 61),
        [255, 255, 255, 255],
        "just past the square"
    );

    // A scale that does not land on whole pixels rounds the size.
    let odd = image(&doc, 0.55, false).unwrap();
    assert_eq!((odd.width, odd.height), (44, 33));
}

#[test]
fn a_transparent_export_leaves_out_the_page_in_straight_alpha() {
    let clear = image(&document(), 1.0, true).unwrap();
    assert_eq!(pixel(&clear, 20, 20), [255, 0, 0, 255]);
    assert_eq!(pixel(&clear, 70, 50)[3], 0, "nothing where the page was");
    // Not premultiplied: half-transparent red is still full red.
    let [r, g, b, a] = pixel(&clear, 50, 20);
    assert!(
        r >= 254 && g == 0 && b == 0 && (127..=128).contains(&a),
        "{:?}",
        [r, g, b, a]
    );
}

#[test]
fn impossible_sizes_are_refused() {
    let doc = document();
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, 0.001, 1000.0] {
        assert!(
            matches!(
                image(&doc, scale, false),
                Err(RenderError::ExportSize { .. })
            ),
            "{scale}"
        );
    }
}
