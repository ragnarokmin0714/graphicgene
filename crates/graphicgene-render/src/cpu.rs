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
//! allocates nothing here once warmed up — except a dashed stroke's pattern
//! and a gradient's stops, which tiny-skia takes as `Vec`s.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::geom::{Affine, Bounds, PathEl, Point};
use graphicgene_core::node::{LineCap, LineJoin};
use graphicgene_core::paint::{Gradient, GradientKind, Paint as Fill};
use tiny_skia::{
    Color, FillRule, GradientStop, LineCap as SkLineCap, LineJoin as SkLineJoin, LinearGradient,
    Paint, PathBuilder, Pixmap, PixmapMut, Point as SkPoint, RadialGradient, Rect as SkRect,
    Shader, SpreadMode, Stroke as SkStroke, StrokeDash, Transform,
};

use crate::RenderError;
use crate::renderer::Renderer;
use crate::scene::{RenderItem, RenderScene, device_area};

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
    /// cleared to the scene's background, then the artboard and every item
    /// that reaches it are drawn through `view`, back to front. Pixels
    /// outside it are left alone.
    fn render(
        &mut self,
        scene: &RenderScene,
        view: Affine,
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
            draw_scene(&mut self.builder, scene, view, whole, &mut canvas);
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
            draw_scene(&mut self.builder, scene, view, area, &mut canvas);
        }

        let data = target.data_mut();
        for y in rows {
            let span = y * stride + x0..y * stride + x1;
            data[span.clone()].copy_from_slice(&self.scratch[span]);
        }
        Ok(())
    }
}

/// Everything reaching into `area` (device pixels), back to front: the
/// artboard, then the items, all drawn through `view`.
fn draw_scene(
    builder: &mut PathBuilder,
    scene: &RenderScene,
    view: Affine,
    area: Bounds,
    canvas: &mut PixmapMut,
) {
    if let Some(artboard) = scene.artboard
        && intersects(device_area(view, artboard.rect), area)
    {
        let mut paint = Paint {
            anti_alias: true,
            ..Default::default()
        };
        paint.set_color(to_sk_color(artboard.fill, 1.0));
        let [_, b, c, ..] = view.as_coeffs();
        if b == 0.0 && c == 0.0 {
            // An unrotated view keeps the artboard a device-space rect, so
            // only its overlap with `area` is filled: the scratch buffer is
            // full-size, and filling the whole page for every small redraw
            // cost more than the redraw. `area`'s edges sit on whole pixels,
            // so cutting there changes no pixel's coverage.
            let page = view.transform_rect_bbox(artboard.rect).intersect(area);
            if let Some(rect) = SkRect::from_ltrb(
                page.x0 as f32,
                page.y0 as f32,
                page.x1 as f32,
                page.y1 as f32,
            ) {
                canvas.fill_rect(rect, &paint, Transform::identity(), None);
            }
        } else if let Some(rect) = SkRect::from_ltrb(
            artboard.rect.x0 as f32,
            artboard.rect.y0 as f32,
            artboard.rect.x1 as f32,
            artboard.rect.y1 as f32,
        ) {
            canvas.fill_rect(rect, &paint, to_sk_transform(view), None);
        }
    }
    for item in &scene.items {
        if intersects(device_area(view, item.bounds), area) {
            draw(builder, item, view, canvas);
        }
    }
}

/// Shift `target`'s pixels by (`dx`, `dy`) — what a pan does to a picture
/// that has not otherwise changed — and return the strips left uncovered,
/// which the caller must redraw. A shift of a whole dimension or more leaves
/// nothing to reuse, and returns the whole target.
pub fn scroll(target: &mut Pixmap, dx: i32, dy: i32) -> Vec<PixelRect> {
    let (width, height) = (target.width() as i32, target.height() as i32);
    if dx.abs() >= width || dy.abs() >= height {
        let whole = Bounds::new(0.0, 0.0, f64::from(width), f64::from(height));
        return PixelRect::covering(whole, width as u32, height as u32)
            .into_iter()
            .collect();
    }
    let stride = width as usize * 4;
    let span = (width - dx.abs()) as usize * 4;
    let (from_x, to_x) = if dx >= 0 {
        (0, dx as usize * 4)
    } else {
        (dx.unsigned_abs() as usize * 4, 0)
    };
    let data = target.data_mut();
    let mut copy_row = |y: i32| {
        let source = y - dy;
        if (0..height).contains(&source) {
            let from = source as usize * stride + from_x;
            data.copy_within(from..from + span, y as usize * stride + to_x);
        }
    };
    // Walk the rows against the direction of travel, so none is overwritten
    // before it has been copied.
    if dy > 0 {
        (0..height).rev().for_each(&mut copy_row);
    } else {
        (0..height).for_each(&mut copy_row);
    }

    let (w, h) = (width as u32, height as u32);
    let mut exposed = Vec::with_capacity(2);
    match dy.signum() {
        1 => exposed.push(PixelRect {
            x: 0,
            y: 0,
            width: w,
            height: dy as u32,
        }),
        -1 => exposed.push(PixelRect {
            x: 0,
            y: (height + dy) as u32,
            width: w,
            height: dy.unsigned_abs(),
        }),
        _ => {}
    }
    match dx.signum() {
        1 => exposed.push(PixelRect {
            x: 0,
            y: 0,
            width: dx as u32,
            height: h,
        }),
        -1 => exposed.push(PixelRect {
            x: (width + dx) as u32,
            y: 0,
            width: dx.unsigned_abs(),
            height: h,
        }),
        _ => {}
    }
    exposed
}

fn draw(builder: &mut PathBuilder, item: &RenderItem, view: Affine, canvas: &mut PixmapMut) {
    let mut path = std::mem::take(builder);
    append_path(&mut path, &item.path);
    let Some(path) = path.finish() else {
        // An empty or degenerate path: nothing to draw, and nothing to reuse.
        return;
    };

    // Composed in f64 before narrowing, so zooming does not compound the
    // f32 rounding of two separate transforms.
    let transform = to_sk_transform(view * item.transform);

    if let Some(fill) = &item.fill {
        let mut paint = Paint {
            anti_alias: true,
            ..Default::default()
        };
        match fill {
            Fill::Solid(color) => paint.set_color(to_sk_color(*color, item.opacity)),
            Fill::Gradient(gradient) => {
                paint.shader = gradient_shader(gradient, item.paint_box, item.opacity);
            }
        }
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
            line_cap: match stroke.cap {
                LineCap::Butt => SkLineCap::Butt,
                LineCap::Round => SkLineCap::Round,
                LineCap::Square => SkLineCap::Square,
            },
            line_join: match stroke.join {
                LineJoin::Miter => SkLineJoin::Miter,
                LineJoin::Round => SkLineJoin::Round,
                LineJoin::Bevel => SkLineJoin::Bevel,
            },
            // The one allocation in a draw, and only for dashed strokes:
            // tiny-skia takes the pattern as a Vec. None for a pattern it
            // cannot draw (no length at all), which then draws solid.
            dash: stroke
                .dash
                .and_then(|d| StrokeDash::new(vec![d.length as f32, d.gap as f32], 0.0)),
            ..Default::default()
        };
        canvas.stroke_path(&path, &paint, &sk_stroke, transform, None);
    }

    *builder = path.clear();
}

fn to_sk_transform(affine: Affine) -> Transform {
    let [a, b, c, d, e, f] = affine.as_coeffs().map(|v| v as f32);
    Transform::from_row(a, b, c, d, e, f)
}

/// tiny-skia works in 8-bit sRGB; the document works in linear f32.
/// This conversion is the boundary, and it only ever runs outward.
/// A gradient as tiny-skia draws it. The gradient lives in the unit square
/// of the path's box; the shader's transform stretches that square over the
/// box, and the fill's transform takes it on to the device, as it does the
/// path. Its stops are the one allocation in such a draw.
///
/// A box with no width or height cannot hold a gradient, and tiny-skia
/// declines one there; the last colour fills it instead, which is what
/// tiny-skia does for any gradient that collapses to a point.
fn gradient_shader(gradient: &Gradient, paint_box: Bounds, opacity: f32) -> Shader<'static> {
    let stops: Vec<GradientStop> = gradient
        .stops
        .iter()
        .map(|stop| GradientStop::new(stop.offset as f32, to_sk_color(stop.color, opacity)))
        .collect();
    let last = gradient
        .stops
        .last()
        .map_or(LinearRgba::TRANSPARENT, |s| s.color);
    let unit = Transform::from_row(
        paint_box.width() as f32,
        0.0,
        0.0,
        paint_box.height() as f32,
        paint_box.x0 as f32,
        paint_box.y0 as f32,
    );
    let point = |p: Point| SkPoint::from_xy(p.x as f32, p.y as f32);
    let (start, end) = (point(gradient.start), point(gradient.end));
    let shader = match gradient.kind {
        GradientKind::Linear => LinearGradient::new(start, end, stops, SpreadMode::Pad, unit),
        GradientKind::Radial => {
            let radius = (gradient.end - gradient.start).hypot() as f32;
            RadialGradient::new(start, 0.0, start, radius, stops, SpreadMode::Pad, unit)
        }
    };
    shader.unwrap_or(Shader::SolidColor(to_sk_color(last, opacity)))
}

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
