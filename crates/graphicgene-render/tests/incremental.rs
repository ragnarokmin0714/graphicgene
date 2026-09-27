//! Incremental redraw: the scene updates from the document's change log and
//! only the damaged area is redrawn. Its one obligation is to end up with
//! exactly the pixels a full redraw would produce — not nearly: any
//! difference shows as a seam where a region was redrawn, and accumulates.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Affine, Ellipse, Rect, Shape};
use graphicgene_core::node::{Node, NodeId, NodeKind, Stroke};
use graphicgene_render::{CpuRenderer, Damage, PixelRect, RenderScene, Renderer};
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

fn shape(rng: &mut Rng) -> Node {
    let (x, y) = (rng.range(-20.0, 150.0), rng.range(-20.0, 110.0));
    let (w, h) = (rng.range(4.0, 50.0), rng.range(4.0, 40.0));
    let path = if rng.below(2) == 0 {
        Rect::new(x, y, x + w, y + h).to_path(0.1)
    } else {
        Ellipse::new((x, y), (w / 2.0, h / 2.0), 0.0).to_path(0.1)
    };
    let mut node = Node::vector("Shape", path, Some(rng.colour()));
    if rng.below(3) == 0
        && let NodeKind::Vector(v) = &mut node.kind
    {
        v.stroke = Some(Stroke {
            color: rng.colour(),
            width: rng.range(0.5, 8.0),
        });
    }
    node
}

fn attach(doc: &mut Document, parent: NodeId, node: Node) -> NodeId {
    let id = doc.insert_detached(node);
    let index = doc.children_of(parent).unwrap().len();
    doc.attach(id, parent, index).unwrap();
    id
}

fn full_render(doc: &Document, width: u32, height: u32) -> Pixmap {
    let mut scene = RenderScene::build(doc).unwrap();
    scene.background = Some(LinearRgba::WHITE);
    let mut pixmap = Pixmap::new(width, height).unwrap();
    CpuRenderer::new()
        .render(
            &scene,
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
    random_edits(160, 120, 400, 0x9E37_79B9_7F4A_7C15);
    random_edits(640, 480, 120, 0xD1B5_4A32_D192_ED03);
}

fn random_edits(width: u32, height: u32, steps: usize, seed: u64) {
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
    scene.background = Some(LinearRgba::WHITE);
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
                    v.fill = Some(rng.colour());
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
                Some(region)
            }
            Damage::Everything => Some(full),
        };
        if let Some(dirty) = dirty {
            renderer.render(&scene, dirty, &mut pixels).unwrap();
        }

        let reference = full_render(&doc, width, height);
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
    assert!(region.contains((0.0, 0.0)) && region.contains((60.0, 10.0)));
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
        v.stroke = Some(Stroke {
            color: LinearRgba::BLACK,
            width: 10.0,
        });
    }
    attach(&mut doc, root, node);
    let scene = RenderScene::build(&doc).unwrap();
    let b = scene.items[0].bounds;
    // Half the width, times a miter limit of 4, plus a pixel of antialiasing.
    assert!(
        b.x0 <= 20.0 - 20.0 - 1.0 && b.x1 >= 30.0 + 20.0 + 1.0,
        "{b:?}"
    );
}

#[test]
fn a_partial_redraw_leaves_the_rest_of_the_target_alone() {
    let mut scene = RenderScene::default();
    scene.background = Some(LinearRgba::WHITE);
    let mut pixmap = Pixmap::new(W, H).unwrap();
    pixmap.fill(tiny_skia::Color::from_rgba8(0, 128, 0, 255));
    CpuRenderer::new()
        .render(&scene, Rect::new(10.2, 10.7, 20.1, 20.0), &mut pixmap)
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
