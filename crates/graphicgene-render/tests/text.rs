//! Text is drawn like any path, from what the layout pass set: its glyph
//! outlines, in its fill.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::doc::Document;
use graphicgene_core::fonts::Fonts;
use graphicgene_core::geom::{Affine, Size, Vec2};
use graphicgene_core::layout;
use graphicgene_core::node::{Node, NodeKind};
use graphicgene_core::testing::TestFont;
use graphicgene_core::text::{TextNode, TextStyle};
use graphicgene_render::export::{Image, image};
use graphicgene_render::{CpuRenderer, Damage, RenderScene, Renderer, device_area};
use tiny_skia::Pixmap;

const RED: LinearRgba = LinearRgba::new(1.0, 0.0, 0.0, 1.0);

fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * image.width + x) * 4) as usize;
    image.pixels[at..at + 4].try_into().unwrap()
}

#[test]
fn text_is_drawn_from_its_layout_and_redrawn_when_it_changes() {
    let mut fonts = Fonts::default();
    fonts
        .add(TestFont::new("T").glyphs("AB", 600).build())
        .unwrap();
    let mut doc = Document::with_artboard(Size::new(100.0, 60.0));
    let style = TextStyle {
        family: "T".into(),
        size: 40.0,
        ..TextStyle::default()
    };
    let mut node = Node::text("AB", TextNode::new("AB", style, Some(RED)));
    node.common.transform = Affine::translate(Vec2::new(10.0, 10.0));
    let id = doc.insert_detached(node);
    let root = doc.root();
    doc.attach(id, root, 0).unwrap();

    // Before any layout there is nothing to draw.
    assert_eq!(
        pixel(&image(&doc, 1.0, false).unwrap(), 20, 30),
        [255, 255, 255, 255]
    );

    let mut seen = 0;
    layout::run(&mut doc, &fonts, &mut seen).unwrap();
    // Glyph boxes run 50 units in at 0.04 units a pixel, from x = 12; the
    // baseline is at 10 + (48 - 40) / 2 + 32 = 46, and they stand 28 tall.
    let drawn = image(&doc, 1.0, false).unwrap();
    assert_eq!(pixel(&drawn, 20, 30), [255, 0, 0, 255], "inside the A");
    assert_eq!(
        pixel(&drawn, 20, 50),
        [255, 255, 255, 255],
        "below the baseline"
    );

    // Changing the text damages where it was and where it is, and a partial
    // redraw of that equals a full one.
    let mut scene = RenderScene::build(&doc).unwrap();
    doc.take_changes();
    let mut shown = Pixmap::new(100, 60).unwrap();
    let mut renderer = CpuRenderer::new();
    let full = graphicgene_core::geom::Rect::new(0.0, 0.0, 100.0, 60.0);
    renderer
        .render(&scene, Affine::IDENTITY, full, &mut shown)
        .unwrap();

    if let NodeKind::Text(text) = &mut doc.get_mut(id).unwrap().kind {
        text.content = "BA\nA".into();
    }
    layout::run(&mut doc, &fonts, &mut seen).unwrap();
    let changes = doc.take_changes();
    let Damage::Region(region) = scene.update(&doc, &changes).unwrap() else {
        panic!("only the text changed");
    };
    renderer
        .render(
            &scene,
            Affine::IDENTITY,
            device_area(Affine::IDENTITY, region),
            &mut shown,
        )
        .unwrap();
    let mut fresh = Pixmap::new(100, 60).unwrap();
    let rebuilt = RenderScene::build(&doc).unwrap();
    CpuRenderer::new()
        .render(&rebuilt, Affine::IDENTITY, full, &mut fresh)
        .unwrap();
    assert_eq!(shown.data(), fresh.data());
}
