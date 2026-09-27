//! CPU renderer, backed by `tiny-skia`.
//!
//! The hard part of vector rendering is tessellation and antialiasing, not
//! reaching the GPU, and `tiny-skia` is the rasterizer behind `resvg` which PNG
//! export will need anyway. See CLAUDE.md for why v0.1 is CPU-first.
//!
//! A redraw covers only the dirty rect it is given. Only the items reaching
//! into that rect are drawn, into a scratch buffer, and only the rect is
//! copied into the target, row by row.
//!
//! The scratch buffer is the target's full size, not the dirty rect's, and
//! that is deliberate. tiny-skia chops curves where they cross the edge of
//! the buffer, and where a curve is chopped changes how it is flattened —
//! so a curve drawn into a rect-sized buffer comes out up to ~20/255 off
//! along its antialiased edge compared with a full redraw, leaving faint
//! seams wherever a region was redrawn. Drawing at the full size keeps every
//! edge exactly as a full redraw draws it; the cost stays proportional to
//! the items touched, since the rest of the scratch is never read. The
//! scratch buffer and the path builder are reused, so a steady drag
//! allocates nothing here once warmed up.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::geom::{Bounds, PathEl};
use tiny_skia::{
    Color, FillRule, Paint, PathBuilder, Pixmap, PixmapMut, Stroke as SkStroke, Transform,
};

use crate::RenderError;
use crate::renderer::Renderer;
use crate::scene::{RenderItem, RenderScene};

#[derive(Debug, Default)]
pub struct CpuRenderer {
    /// Handed back by each finished path (`Path::clear`), so its allocation
    /// is reused by the next one.
    builder: PathBuilder,
    /// Backing store for partial redraws, the size of the target. Only the
    /// dirty rect of it is ever cleared or read.
    scratch: Vec<u8>,
}

impl CpuRenderer {
    pub fn new() -> Self {
        Self::default()
    }
}

/// A rectangle of whole pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    /// The whole pixels that `area` touches, clipped to a `width` × `height`
    /// target. `None` if nothing is left.
    pub fn covering(area: Bounds, width: u32, height: u32) -> Option<Self> {
        // Before clamping: `max` would quietly turn a NaN into 0.
        if !(area.x0 < area.x1 && area.y0 < area.y1) {
            return None;
        }
        let x0 = area.x0.floor().max(0.0);
        let y0 = area.y0.floor().max(0.0);
        let x1 = area.x1.ceil().min(f64::from(width));
        let y1 = area.y1.ceil().min(f64::from(height));
        if !(x1 > x0 && y1 > y0) {
            return None;
        }
        Some(Self {
            x: x0 as u32,
            y: y0 as u32,
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
        })
    }

    pub fn bounds(self) -> Bounds {
        Bounds::new(
            f64::from(self.x),
            f64::from(self.y),
            f64::from(self.x + self.width),
            f64::from(self.y + self.height),
        )
    }
}

impl Renderer for CpuRenderer {
    type Target = Pixmap;
    type Error = RenderError;

    /// Redraw the part of `target` that `dirty` touches, from scratch: it is
    /// cleared to the scene's background, then every item that reaches it is
    /// drawn, back to front. Pixels outside it are left alone.
    fn render(
        &mut self,
        scene: &RenderScene,
        dirty: Bounds,
        target: &mut Pixmap,
    ) -> Result<(), RenderError> {
        let (width, height) = (target.width(), target.height());
        let Some(rect) = PixelRect::covering(dirty, width, height) else {
            return Ok(());
        };
        let background = scene
            .background
            .map_or(Color::TRANSPARENT, |c| to_sk_color(c, 1.0));
        let area = rect.bounds();

        // Past half the target, clearing and copying back through the scratch
        // buffer costs more than it saves: redraw the whole target directly.
        let rect_area = u64::from(rect.width) * u64::from(rect.height);
        if rect_area * 2 >= u64::from(width) * u64::from(height) {
            // Everything is cleared, so everything on the target is drawn —
            // not just what reaches the dirty rect.
            let whole = Bounds::new(0.0, 0.0, f64::from(width), f64::from(height));
            target.fill(background);
            let mut canvas = target.as_mut();
            for item in scene.items.iter().filter(|i| intersects(i.bounds, whole)) {
                draw(&mut self.builder, item, &mut canvas);
            }
            return Ok(());
        }

        let stride = width as usize * 4;
        let len = stride * height as usize;
        if self.scratch.len() < len {
            self.scratch.resize(len, 0);
        }
        let (x0, x1) = (rect.x as usize * 4, (rect.x + rect.width) as usize * 4);
        let rows = rect.y as usize..(rect.y + rect.height) as usize;

        // Clear just the rect to the background; the rest of the scratch
        // buffer may hold anything, since it is never copied out.
        let fill = background.premultiply().to_color_u8();
        let fill = [fill.red(), fill.green(), fill.blue(), fill.alpha()];
        for y in rows.clone() {
            let (pixels, _) = self.scratch[y * stride + x0..y * stride + x1].as_chunks_mut::<4>();
            pixels.fill(fill);
        }
        {
            let mut canvas = PixmapMut::from_bytes(&mut self.scratch[..len], width, height)
                .ok_or(RenderError::BadTargetSize { width, height })?;
            for item in scene.items.iter().filter(|i| intersects(i.bounds, area)) {
                draw(&mut self.builder, item, &mut canvas);
            }
        }

        let data = target.data_mut();
        for y in rows {
            let span = y * stride + x0..y * stride + x1;
            data[span.clone()].copy_from_slice(&self.scratch[span]);
        }
        Ok(())
    }
}

fn draw(builder: &mut PathBuilder, item: &RenderItem, canvas: &mut PixmapMut) {
    let mut path = std::mem::take(builder);
    append_path(&mut path, &item.path);
    let Some(path) = path.finish() else {
        // An empty or degenerate path: nothing to draw, and nothing to reuse.
        return;
    };

    let [a, b, c, d, e, f] = item.transform.as_coeffs().map(|v| v as f32);
    let transform = Transform::from_row(a, b, c, d, e, f);

    if let Some(fill) = item.fill {
        let mut paint = Paint {
            anti_alias: true,
            ..Default::default()
        };
        paint.set_color(to_sk_color(fill, item.opacity));
        canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
    }

    if let Some(stroke) = item.stroke {
        let mut paint = Paint {
            anti_alias: true,
            ..Default::default()
        };
        paint.set_color(to_sk_color(stroke.color, item.opacity));
        let sk_stroke = SkStroke {
            width: stroke.width as f32,
            ..Default::default()
        };
        canvas.stroke_path(&path, &paint, &sk_stroke, transform, None);
    }

    *builder = path.clear();
}

/// tiny-skia works in 8-bit sRGB; the document works in linear f32.
/// This conversion is the boundary, and it only ever runs outward.
fn to_sk_color(color: LinearRgba, opacity: f32) -> tiny_skia::Color {
    let [r, g, b, a] = LinearRgba {
        a: color.a * opacity.clamp(0.0, 1.0),
        ..color
    }
    .to_srgb8();
    tiny_skia::Color::from_rgba8(r, g, b, a)
}

fn append_path(builder: &mut PathBuilder, path: &graphicgene_core::geom::BezPath) {
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => builder.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => builder.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(c, p) => builder.quad_to(c.x as f32, c.y as f32, p.x as f32, p.y as f32),
            PathEl::CurveTo(c1, c2, p) => builder.cubic_to(
                c1.x as f32,
                c1.y as f32,
                c2.x as f32,
                c2.y as f32,
                p.x as f32,
                p.y as f32,
            ),
            PathEl::ClosePath => builder.close(),
        }
    }
}

fn intersects(a: Bounds, b: Bounds) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}
