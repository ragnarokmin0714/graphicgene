//! Outline view: every path a one-pixel line, nothing filled.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Affine, Rect, Shape, Size};
use graphicgene_core::node::{Node, NodeKind, Stroke};
use graphicgene_render::cpu::CpuRenderer;
use graphicgene_render::renderer::Renderer;
use graphicgene_render::scene::RenderScene;
use tiny_skia::Pixmap;

const RED: LinearRgba = LinearRgba::new(1.0, 0.0, 0.0, 1.0);

/// A red square from 20 to 80, stroked 6 wide in blue, on a 100 × 100
/// artboard, drawn at 1:1.
fn render(outline: bool) -> Pixmap {
    let mut doc = Document::with_artboard(Size::new(100.0, 100.0));
    let root = doc.root();
    let mut node = Node::vector(
        "Square",
        Rect::new(20.0, 20.0, 80.0, 80.0).to_path(0.1),
        Some(RED),
    );
    if let NodeKind::Vector(v) = &mut node.kind {
        v.stroke = Some(Stroke::solid(LinearRgba::new(0.0, 0.0, 1.0, 1.0), 6.0));
    }
    let id = doc.insert_detached(node);
    doc.attach(id, root, 0).unwrap();
    let mut scene = RenderScene::build(&doc).unwrap();
    scene.outline = outline;
    let mut pixmap = Pixmap::new(100, 100).unwrap();
    let whole = Rect::new(0.0, 0.0, 100.0, 100.0);
    CpuRenderer::new()
        .render(&scene, Affine::IDENTITY, whole, &mut pixmap)
        .unwrap();
    pixmap
}

fn rgb(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 3] {
    let p = pixmap.pixel(x, y).unwrap();
    [p.red(), p.green(), p.blue()]
}

#[test]
fn outline_view_draws_paths_as_hairlines_and_fills_nothing() {
    let normal = render(false);
    assert_eq!(rgb(&normal, 50, 50), [255, 0, 0], "filled red");
    assert_eq!(rgb(&normal, 18, 50), [0, 0, 255], "stroked blue, 3 out");

    let outline = render(true);
    assert_eq!(
        rgb(&outline, 50, 50),
        [255, 255, 255],
        "the page shows through"
    );
    assert_eq!(rgb(&outline, 17, 50), [255, 255, 255], "no stroke width");
    // The line x = 20 runs between pixel columns 19 and 20, so a hairline
    // there half-covers both.
    let [r, g, b] = rgb(&outline, 19, 50);
    assert!(
        r < 200 && r == g && g == b,
        "a dark line on the edge: {r} {g} {b}"
    );
}
