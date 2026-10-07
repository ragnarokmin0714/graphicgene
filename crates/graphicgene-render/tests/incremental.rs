//! Incremental redraw: the scene updates from the document's change log and
//! only the damaged area is redrawn. Its one obligation is to end up with
//! exactly the pixels a full redraw would produce — not nearly: any
//! difference shows as a seam where a region was redrawn, and accumulates.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Affine, BezPath, Ellipse, Point, Rect, Shape};
use graphicgene_core::node::{Dash, LineCap, LineJoin, Node, NodeId, NodeKind, Stroke};
use graphicgene_core::paint::{ColorStop, Gradient, GradientKind, Paint};
use graphicgene_render::{
    CpuRenderer, Damage, PixelRect, RenderScene, Renderer, device_area, scroll,
};
use tiny_skia::Pixmap;

/// The small target, for the tests that need only one.
const W: u32 = 160;
const H: u32 = 120;

/// xorshift64: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() % 10_000) as f64 / 10_000.0 * (hi - lo)
    }
    fn colour(&mut self) -> LinearRgba {
        let [r, g, b] = [0, 0, 0].map(|_: u8| self.below(256) as u8);
        LinearRgba::from_srgb8(r, g, b, 255)
    }
}

/// A solid colour or, as often, a linear or radial gradient of two or three
/// stops pointing anywhere in the shape's box — partly beyond it, too.
fn fill(rng: &mut Rng) -> Paint {
    if rng.below(2) == 0 {
        return Paint::Solid(rng.colour());
    }
    let kind = if rng.below(2) == 0 {
        GradientKind::Linear
    } else {
        GradientKind::Radial
    };
    let count = 2 + rng.below(2) as usize;
    let stops = (0..count)
        .map(|i| ColorStop {
            offset: i as f64 / (count - 1) as f64,
            color: rng.colour(),
        })
        .collect();
    let mut gradient = Gradient::new(kind, stops);
    gradient.start = Point::new(rng.range(-0.2, 1.2), rng.range(-0.2, 1.2));
    gradient.end = Point::new(rng.range(-0.2, 1.2), rng.range(-0.2, 1.2));
    Paint::Gradient(gradient)
}

fn shape(rng: &mut Rng) -> Node {
    let (x, y) = (rng.range(-20.0, 150.0), rng.range(-20.0, 110.0));
    let (w, h) = (rng.range(4.0, 50.0), rng.range(4.0, 40.0));
    let path = match rng.below(4) {
        0 => Rect::new(x, y, x + w, y + h).to_path(0.1),
        1 => Ellipse::new((x, y), (w / 2.0, h / 2.0), 0.0).to_path(0.1),
        // A sharp triangle: its tip is where a miter join reaches furthest.
        2 => {
            let mut path = BezPath::new();
            path.move_to((x, y));
            path.line_to((x + w, y + h * 0.1));
            path.line_to((x, y + h * 0.2));
            path.close_path();
            path
        }
        // An open zigzag, whose ends show the caps.
        _ => {
            let mut path = BezPath::new();
            path.move_to((x, y));
            path.line_to((x + w / 2.0, y + h));
            path.line_to((x + w, y));
            path
        }
    };
    let mut node = Node::vector("Shape", path, None);
    if let NodeKind::Vector(v) = &mut node.kind {
        v.fill = Some(fill(rng));
    }
    if rng.below(3) == 0
        && let NodeKind::Vector(v) = &mut node.kind
    {
        let mut stroke = Stroke::solid(rng.colour(), rng.range(0.5, 8.0));
        stroke.cap = [LineCap::Butt, LineCap::Round, LineCap::Square][rng.below(3) as usize];
        stroke.join = [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel][rng.below(3) as usize];
        if rng.below(2) == 0 {
            stroke.dash = Some(Dash {
                length: rng.range(1.0, 10.0),
                gap: rng.range(1.0, 10.0),
            });
        }
        v.stroke = Some(stroke);
    }
    node
}

fn attach(doc: &mut Document, parent: NodeId, node: Node) -> NodeId {
    let id = doc.insert_detached(node);
    let index = doc.children_of(parent).unwrap().len();
    doc.attach(id, parent, index).unwrap();
    id
}

/// The grey around the artboard, so a missed redraw of the backdrop shows.
fn backdrop() -> LinearRgba {
    LinearRgba::from_srgb8(200, 200, 200, 255)
}

fn full_render(doc: &Document, view: Affine, width: u32, height: u32) -> Pixmap {
    let mut scene = RenderScene::build(doc).unwrap();
    scene.background = Some(backdrop());
    let mut pixmap = Pixmap::new(width, height).unwrap();
    CpuRenderer::new()
        .render(
            &scene,
            view,
            Rect::new(0.0, 0.0, width.into(), height.into()),
            &mut pixmap,
        )
        .unwrap();
    pixmap
}

/// Largest per-channel difference between two pixmaps.
fn max_difference(a: &Pixmap, b: &Pixmap) -> u8 {
    a.data()
        .iter()
        .zip(b.data())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

/// On a small target most damaged areas cover over half of it, which the
/// renderer promotes to a full redraw; on a large one they stay partial and
/// go through the scratch buffer. Both paths must match a full redraw.
#[test]
fn incremental_redraws_match_a_full_redraw_through_random_edits() {
    let zoomed = Affine::translate((23.0, -17.0)) * Affine::scale(1.5);
    random_edits(160, 120, 400, 0x9E37_79B9_7F4A_7C15, Affine::IDENTITY);
    random_edits(640, 480, 120, 0xD1B5_4A32_D192_ED03, Affine::IDENTITY);
    random_edits(640, 480, 120, 0x2545_F491_4F6C_DD1D, zoomed);
}

fn random_edits(width: u32, height: u32, steps: usize, seed: u64, view: Affine) {
    let mut rng = Rng(seed);
    let mut doc = Document::new();
    let root = doc.root();
    let mut nodes: Vec<NodeId> = (0..24)
        .map(|_| attach(&mut doc, root, shape(&mut rng)))
        .collect();
    let group = attach(&mut doc, root, Node::group("Group"));
    for _ in 0..6 {
        let child = shape(&mut rng);
        nodes.push(attach(&mut doc, group, child));
    }
    nodes.push(group);

    let mut scene = RenderScene::default();
    scene.background = Some(backdrop());
    let mut renderer = CpuRenderer::new();
    let mut pixels = Pixmap::new(width, height).unwrap();
    let full = Rect::new(0.0, 0.0, width.into(), height.into());
    let (mut partial_frames, mut quiet_frames) = (0, 0);

    for step in 0..steps {
        let id = nodes[rng.below(nodes.len() as u64) as usize];
        match rng.below(9) {
            0 | 1 => {
                let t = Affine::translate((rng.range(-30.0, 30.0), rng.range(-30.0, 30.0)))
                    * Affine::rotate(rng.range(-0.5, 0.5))
                    * Affine::scale(rng.range(0.6, 1.5));
                doc.get_mut(id).unwrap().common.transform = t;
            }
            2 => {
                if let NodeKind::Vector(v) = &mut doc.get_mut(id).unwrap().kind {
                    v.fill = Some(fill(&mut rng));
                }
            }
            3 => {
                let node = doc.get_mut(id).unwrap();
                node.common.visible = !node.common.visible;
            }
            4 => {
                // Includes 0: a node that disappears without being hidden.
                doc.get_mut(id).unwrap().common.opacity =
                    [0.0, 0.3, 0.7, 1.0][rng.below(4) as usize];
            }
            5 => {
                let fresh = shape(&mut rng);
                if let (NodeKind::Vector(v), NodeKind::Vector(f)) =
                    (&mut doc.get_mut(id).unwrap().kind, fresh.kind)
                {
                    v.path = f.path;
                    v.stroke = f.stroke;
                }
            }
            6 => {
                let fresh = shape(&mut rng);
                nodes.push(attach(&mut doc, root, fresh));
            }
            7 if doc.is_attached(id) && id != group => {
                doc.detach(id).unwrap();
            }
            _ => {
                // Nothing changed on screen: e.g. a selection click.
            }
        }

        let changes = doc.take_changes();
        let damage = scene.update(&doc, &changes).unwrap();
        let dirty = match damage {
            Damage::None => {
                quiet_frames += 1;
                None
            }
            Damage::Region(region) => {
                partial_frames += 1;
                Some(device_area(view, region))
            }
            Damage::Everything => Some(full),
        };
        if let Some(dirty) = dirty {
            renderer.render(&scene, view, dirty, &mut pixels).unwrap();
        }

        let reference = full_render(&doc, view, width, height);
        let diff = max_difference(&pixels, &reference);
        assert_eq!(
            diff, 0,
            "step {step}: incremental redraw differs from a full redraw ({damage:?})"
        );
    }
    // Structural edits and nodes appearing or vanishing redraw everything;
    // the rest must stay partial, or the test proves nothing about regions.
    assert!(
        partial_frames > steps / 5,
        "too few partial redraws: {partial_frames}"
    );
    assert!(
        quiet_frames > steps / 20,
        "too few no-op frames: {quiet_frames}"
    );
}

#[test]
fn moving_a_shape_damages_where_it_was_and_where_it_is() {
    let mut doc = Document::new();
    let root = doc.root();
    let id = attach(
        &mut doc,
        root,
        Node::vector(
            "R",
            Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1),
            Some(LinearRgba::BLACK),
        ),
    );
    let mut scene = RenderScene::build(&doc).unwrap();
    doc.take_changes();

    doc.get_mut(id).unwrap().common.transform = Affine::translate((50.0, 0.0));
    let changes = doc.take_changes();
    let Damage::Region(region) = scene.update(&doc, &changes).unwrap() else {
        panic!("expected a region");
    };
    // (Rect::contains excludes the far edges, so probe just inside them.)
    assert!(region.contains((0.5, 0.5)) && region.contains((59.5, 9.5)));
    assert!(
        !region.contains((30.0, 40.0)),
        "and nothing far from either"
    );
}

#[test]
fn damage_includes_the_whole_stroke() {
    let mut doc = Document::new();
    let root = doc.root();
    let mut node = Node::vector("R", Rect::new(20.0, 20.0, 30.0, 30.0).to_path(0.1), None);
    if let NodeKind::Vector(v) = &mut node.kind {
        v.stroke = Some(Stroke::solid(LinearRgba::BLACK, 10.0));
    }
    attach(&mut doc, root, node);
    let scene = RenderScene::build(&doc).unwrap();
    let b = scene.items[0].bounds;
    // Half the width, times a miter limit of 4.
    assert!(b.x0 <= 20.0 - 20.0 && b.x1 >= 30.0 + 20.0, "{b:?}");
    // Plus a device pixel of antialiasing, whatever the zoom.
    let quarter = device_area(Affine::scale(0.25), b);
    assert!(quarter.x0 <= b.x0 * 0.25 - 1.0, "{quarter:?}");
}

#[test]
fn a_partial_redraw_leaves_the_rest_of_the_target_alone() {
    let mut scene = RenderScene::default();
    scene.background = Some(LinearRgba::WHITE);
    let mut pixmap = Pixmap::new(W, H).unwrap();
    pixmap.fill(tiny_skia::Color::from_rgba8(0, 128, 0, 255));
    CpuRenderer::new()
        .render(
            &scene,
            Affine::IDENTITY,
            Rect::new(10.2, 10.7, 20.1, 20.0),
            &mut pixmap,
        )
        .unwrap();
    let px = |x, y| pixmap.pixel(x, y).unwrap();
    assert_eq!(
        px(15, 15).green(),
        255,
        "inside is cleared to the background"
    );
    assert_eq!(
        px(10, 10).green(),
        255,
        "partly covered pixels count as inside"
    );
    assert_eq!(px(9, 15).green(), 128, "outside is untouched");
    assert_eq!(px(21, 15).green(), 128);
}

#[test]
fn pixel_rects_clip_and_reject_empty_areas() {
    assert_eq!(
        PixelRect::covering(Rect::new(-5.0, 2.5, 3.2, 400.0), 10, 10),
        Some(PixelRect {
            x: 0,
            y: 2,
            width: 4,
            height: 8
        })
    );
    assert_eq!(
        PixelRect::covering(Rect::new(20.0, 0.0, 30.0, 5.0), 10, 10),
        None
    );
    assert_eq!(
        PixelRect::covering(Rect::new(f64::NAN, 0.0, 5.0, 5.0), 10, 10),
        None
    );
}

/// Pan by shifting pixels, redrawing the uncovered strips, and compare with
/// a full redraw at the new position; returns (largest difference, fraction
/// of pixels off by more than 2).
fn pan_against_full_redraws(doc: &Document) -> (u8, f64) {
    let (width, height) = (200, 150);
    let whole = Rect::new(0.0, 0.0, width.into(), height.into());
    let mut scene = RenderScene::build(doc).unwrap();
    scene.background = Some(backdrop());
    let mut renderer = CpuRenderer::new();
    let mut view = Affine::translate((-40.0, 12.0)) * Affine::scale(1.75);
    let mut pixels = Pixmap::new(width, height).unwrap();
    renderer.render(&scene, view, whole, &mut pixels).unwrap();

    let (mut worst, mut worst_share) = (0u8, 0.0f64);
    for (dx, dy) in [(17, -9), (-3, 25), (0, -40), (60, 0), (-199, 3), (500, 0)] {
        view = Affine::translate((f64::from(dx), f64::from(dy))) * view;
        for strip in scroll(&mut pixels, dx, dy) {
            renderer
                .render(&scene, view, strip.bounds(), &mut pixels)
                .unwrap();
        }
        let reference = full_render(doc, view, width, height);
        worst = worst.max(max_difference(&pixels, &reference));
        let off = pixels
            .data()
            .chunks(4)
            .zip(reference.data().chunks(4))
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 2))
            .count();
        worst_share = worst_share.max(off as f64 / f64::from(width * height));
    }
    (worst, worst_share)
}

/// The shifting and strip logic, checked where rasterization is exactly
/// translation-invariant: axis-aligned edges, which the canvas edge cannot
/// change by cutting them.
#[test]
fn panning_by_shifting_pixels_matches_a_redraw_for_axis_aligned_edges() {
    let mut rng = Rng(0xA076_1D64_78BD_642F);
    let mut doc = Document::new();
    let root = doc.root();
    for _ in 0..30 {
        let (x, y) = (
            rng.range(-20.0, 150.0).round(),
            rng.range(-20.0, 110.0).round(),
        );
        let (w, h) = (rng.range(4.0, 50.0).round(), rng.range(4.0, 40.0).round());
        let rect = Rect::new(x, y, x + w, y + h).to_path(0.1);
        attach(
            &mut doc,
            root,
            Node::vector("Rect", rect, Some(rng.colour())),
        );
    }
    let (worst, _) = pan_against_full_redraws(&doc);
    assert!(worst <= 1, "differs by {worst}");
}

/// Any other edge cut by the old canvas edge — a curve, a slanted line — is
/// rasterized from different cut points, so once shifted inward it can be a
/// little off a fresh redraw (seen: up to ~40/255 on antialiased pixels).
/// That is why the shell redraws in full once a pan settles. The difference
/// must stay confined to such edge pixels, never whole strips.
#[test]
fn panning_by_shifting_pixels_is_close_for_curves_and_slants() {
    let mut rng = Rng(0x94D0_49BB_1331_11EB);
    let mut doc = Document::new();
    let root = doc.root();
    for _ in 0..30 {
        let mut node = shape(&mut rng);
        node.common.transform = Affine::rotate(rng.range(-0.5, 0.5));
        attach(&mut doc, root, node);
    }
    let (_, share) = pan_against_full_redraws(&doc);
    assert!(share < 0.005, "{:.2}% of pixels differ", share * 100.0);
}

#[test]
fn scrolling_reports_the_strips_it_uncovered() {
    let mut pixmap = Pixmap::new(10, 8).unwrap();
    let strip = |x, y, width, height| PixelRect {
        x,
        y,
        width,
        height,
    };
    assert_eq!(
        scroll(&mut pixmap, 3, -2),
        [strip(0, 6, 10, 2), strip(0, 0, 3, 8)]
    );
    assert_eq!(scroll(&mut pixmap, 0, 0), [], "no shift, nothing uncovered");
    assert_eq!(
        scroll(&mut pixmap, -10, 0),
        [strip(0, 0, 10, 8)],
        "a whole width: everything"
    );
}

#[test]
fn scrolling_moves_pixels_by_exactly_the_shift() {
    let mut pixmap = Pixmap::new(16, 12).unwrap();
    // A distinct value per pixel, so any misplaced copy shows.
    for (i, px) in pixmap.data_mut().chunks_mut(4).enumerate() {
        px.copy_from_slice(&[(i % 251) as u8, (i / 251) as u8, 0, 255]);
    }
    let before = pixmap.clone();
    let (dx, dy) = (-5, 3);
    scroll(&mut pixmap, dx, dy);
    for y in 0..12i32 {
        for x in 0..16i32 {
            let (sx, sy) = (x - dx, y - dy);
            if (0..16).contains(&sx) && (0..12).contains(&sy) {
                assert_eq!(
                    pixmap.pixel(x as u32, y as u32),
                    before.pixel(sx as u32, sy as u32),
                    "({x}, {y})"
                );
            }
        }
    }
}
