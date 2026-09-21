//! CPU renderer, backed by `tiny-skia`.
//!
//! The hard part of vector rendering is tessellation and antialiasing, not
//! reaching the GPU, and `tiny-skia` is the rasterizer behind `resvg` which PNG
//! export will need anyway. See CLAUDE.md for why v0.1 is CPU-first.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::geom::{Bounds, PathEl};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke as SkStroke, Transform};

use crate::RenderError;
use crate::renderer::Renderer;
use crate::scene::{RenderItem, RenderScene};

#[derive(Debug, Default)]
pub struct CpuRenderer {
    /// Reused across frames; nothing in the draw loop allocates per frame
    /// beyond what tiny-skia needs internally.
    builder: PathBuilder,
}

impl CpuRenderer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Renderer for CpuRenderer {
    type Target = Pixmap;
    type Error = RenderError;

    fn render(
        &mut self,
        scene: &RenderScene,
        dirty: Bounds,
        target: &mut Pixmap,
    ) -> Result<(), RenderError> {
        for item in &scene.items {
            // Dirty-rect culling. v0.1 passes the whole viewport, so this is a
            // no-op in practice, but the path is here and exercised.
            if !intersects(item.bounds, dirty) {
                continue;
            }
            self.draw(item, target)?;
        }
        Ok(())
    }
}

impl CpuRenderer {
    fn draw(&mut self, item: &RenderItem, target: &mut Pixmap) -> Result<(), RenderError> {
        let mut builder = std::mem::take(&mut self.builder);
        builder.clear();
        append_path(&mut builder, &item.path);
        let path = builder.finish();
        // Put the builder back regardless of whether the path was usable.
        self.builder = PathBuilder::new();

        let Some(path) = path else {
            return Ok(());
        };

        let coeffs = item.transform.as_coeffs();
        let transform = Transform::from_row(
            coeffs[0] as f32,
            coeffs[1] as f32,
            coeffs[2] as f32,
            coeffs[3] as f32,
            coeffs[4] as f32,
            coeffs[5] as f32,
        );

        if let Some(fill) = item.fill {
            let mut paint = Paint {
                anti_alias: true,
                ..Default::default()
            };
            paint.set_color(to_sk_color(fill, item.opacity));
            target.fill_path(&path, &paint, FillRule::Winding, transform, None);
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
            target.stroke_path(&path, &paint, &sk_stroke, transform, None);
        }

        Ok(())
    }
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
