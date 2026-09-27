use graphicgene_core::color::LinearRgba;
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Affine, Rect, Shape};
use graphicgene_core::node::Node;
use graphicgene_render::{CpuRenderer, RenderScene, Renderer};
use tiny_skia::Pixmap;

fn doc_with_rect() -> (Document, graphicgene_core::NodeId) {
    let mut doc = Document::new();
    let node = Node::vector(
        "Rect",
        Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1),
        Some(LinearRgba::from_srgb8(255, 0, 0, 255)),
    );
    let id = doc.insert_detached(node);
    let root = doc.root();
    doc.attach(id, root, 0).unwrap();
    (doc, id)
}

#[test]
fn scene_bakes_world_transform_into_items() {
    let (mut doc, id) = doc_with_rect();
    doc.get_mut(id).unwrap().common.transform = Affine::translate((20.0, 30.0));

    let scene = RenderScene::build(&doc).unwrap();
    assert_eq!(scene.items.len(), 1);
    assert_eq!(scene.items[0].transform, Affine::translate((20.0, 30.0)));
    // Bounds are in document space; the antialiasing margin is added in
    // device space, where it is a pixel whatever the zoom.
    let b = scene.items[0].bounds;
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (20.0, 30.0, 30.0, 40.0));
}

#[test]
fn hidden_nodes_never_reach_the_scene() {
    let (mut doc, id) = doc_with_rect();
    doc.get_mut(id).unwrap().common.visible = false;
    assert!(RenderScene::build(&doc).unwrap().items.is_empty());
}

#[test]
fn opacity_accumulates_down_the_tree() {
    let mut doc = Document::new();
    let group = doc.insert_detached(Node::group("G"));
    let root = doc.root();
    doc.attach(group, root, 0).unwrap();
    doc.get_mut(group).unwrap().common.opacity = 0.5;

    let child = doc.insert_detached(Node::vector(
        "Rect",
        Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1),
        Some(LinearRgba::BLACK),
    ));
    doc.attach(child, group, 0).unwrap();
    doc.get_mut(child).unwrap().common.opacity = 0.5;

    let scene = RenderScene::build(&doc).unwrap();
    assert!((scene.items[0].opacity - 0.25).abs() < 1e-6);
}

#[test]
fn cpu_renderer_draws_the_fill() {
    let (doc, _) = doc_with_rect();
    let scene = RenderScene::build(&doc).unwrap();
    let mut pixmap = Pixmap::new(32, 32).unwrap();
    let mut renderer = CpuRenderer::new();

    renderer
        .render(
            &scene,
            Affine::IDENTITY,
            Rect::new(0.0, 0.0, 32.0, 32.0),
            &mut pixmap,
        )
        .unwrap();

    let px = pixmap.pixel(5, 5).unwrap();
    assert_eq!((px.red(), px.green(), px.blue()), (255, 0, 0));
    let outside = pixmap.pixel(25, 25).unwrap();
    assert_eq!(
        (outside.red(), outside.alpha()),
        (255, 255),
        "outside the rect is the artboard's white"
    );
}

#[test]
fn dirty_rect_culls_items_outside_it() {
    let (doc, _) = doc_with_rect();
    let scene = RenderScene::build(&doc).unwrap();
    let mut pixmap = Pixmap::new(32, 32).unwrap();
    let mut renderer = CpuRenderer::new();

    renderer
        .render(
            &scene,
            Affine::IDENTITY,
            Rect::new(20.0, 20.0, 32.0, 32.0),
            &mut pixmap,
        )
        .unwrap();

    assert_eq!(
        pixmap.pixel(5, 5).unwrap().alpha(),
        0,
        "culled, nothing drawn"
    );
}

#[test]
fn the_view_scales_and_moves_everything_including_the_artboard() {
    let (doc, _) = doc_with_rect();
    let mut scene = RenderScene::build(&doc).unwrap();
    scene.background = Some(LinearRgba::BLACK);
    let mut pixmap = Pixmap::new(64, 64).unwrap();
    // 2x zoom with the document origin at (10, 10).
    let view = Affine::translate((10.0, 10.0)) * Affine::scale(2.0);
    CpuRenderer::new()
        .render(&scene, view, Rect::new(0.0, 0.0, 64.0, 64.0), &mut pixmap)
        .unwrap();
    let px = |x, y| {
        let p = pixmap.pixel(x, y).unwrap();
        (p.red(), p.green(), p.blue())
    };
    assert_eq!(px(5, 5), (0, 0, 0), "the backdrop, outside the artboard");
    assert_eq!(px(25, 25), (255, 0, 0), "the rect, now 20 px wide from 10");
    assert_eq!(px(35, 35), (255, 255, 255), "the artboard beyond it");
}
