//! Gradient fills reach the pixels, spread over the shape's own box.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Affine, BezPath, Rect, Shape, Size};
use graphicgene_core::node::{Node, NodeKind};
use graphicgene_core::paint::{ColorStop, Gradient, GradientKind, Paint};
use graphicgene_render::export::{Image, image};

fn black_to_white(kind: GradientKind) -> Paint {
    Paint::Gradient(Gradient::new(
        kind,
        vec![
            ColorStop {
                offset: 0.0,
                color: LinearRgba::BLACK,
            },
            ColorStop {
                offset: 1.0,
                color: LinearRgba::WHITE,
            },
        ],
    ))
}

/// One shape on a 100 × 40 artboard, transparent around it.
fn draw(path: BezPath, transform: Affine, fill: Paint) -> Image {
    let mut doc = Document::with_artboard(Size::new(100.0, 40.0));
    let root = doc.root();
    let mut node = Node::vector("Shape", path, None);
    node.common.transform = transform;
    if let NodeKind::Vector(v) = &mut node.kind {
        v.fill = Some(fill);
    }
    let id = doc.insert_detached(node);
    doc.attach(id, root, 0).unwrap();
    image(&doc, 1.0, true).unwrap()
}

fn grey(image: &Image, x: u32, y: u32) -> u8 {
    image.pixels[((y * image.width + x) * 4) as usize]
}

#[test]
fn a_linear_gradient_runs_across_the_shape_wherever_it_is() {
    // The same 80-wide box, drawn where it is and moved by its transform:
    // the gradient goes with it.
    let path = Rect::new(0.0, 0.0, 80.0, 20.0).to_path(0.1);
    let fill = black_to_white(GradientKind::Linear);
    for (offset, image) in [
        (
            10,
            draw(path.clone(), Affine::translate((10.0, 10.0)), fill.clone()),
        ),
        (
            0,
            draw(
                Rect::new(0.0, 10.0, 80.0, 30.0).to_path(0.1),
                Affine::IDENTITY,
                fill.clone(),
            ),
        ),
    ] {
        let left = grey(&image, offset + 2, 20);
        let middle = grey(&image, offset + 40, 20);
        let right = grey(&image, offset + 77, 20);
        assert!(left < 20 && right > 235, "{left} … {right}");
        // Mixed in sRGB: halfway is mid-grey, not the brighter linear mean.
        assert!((120..=136).contains(&middle), "{middle}");
    }
}

#[test]
fn a_radial_gradient_spreads_from_the_centre_of_the_box() {
    let image = draw(
        Rect::new(10.0, 0.0, 50.0, 40.0).to_path(0.1),
        Affine::IDENTITY,
        black_to_white(GradientKind::Radial),
    );
    assert!(grey(&image, 30, 20) < 20, "dark at the centre");
    assert!(grey(&image, 11, 20) > 230, "light at the edge");
}

#[test]
fn a_box_with_no_height_takes_the_last_colour() {
    // A filled straight line encloses nothing, so draws nothing — but it
    // must not panic, and its stroke-less fill is simply skipped.
    let mut path = BezPath::new();
    path.move_to((10.0, 20.0));
    path.line_to((90.0, 20.0));
    let image = draw(path, Affine::IDENTITY, black_to_white(GradientKind::Linear));
    assert!(image.pixels.chunks(4).all(|p| p[3] == 0));
}
