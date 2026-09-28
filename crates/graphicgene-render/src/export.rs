//! Raster export: the artboard as an image.
//!
//! Drawn by the same renderer as the canvas, so an export shows what the
//! canvas shows at that zoom — the artboard and the artwork on it, nothing
//! around it.
//!
//! What comes out is pixels, not a file format. Encoding them is left to
//! the platform: a browser already has a PNG encoder (`canvas.toBlob`), and
//! carrying one in the wasm would cost it 53 KB gzipped and nine more
//! crates; a native shell can use the `png` crate. Either way the pixels
//! are the same, and saving the file is the app layer's IO.
//!
//! The document is read as it stands, after the layout pass the last frame
//! ran; the pass is a no-op today.

use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Affine, Rect};
use tiny_skia::Pixmap;

use crate::RenderError;
use crate::cpu::CpuRenderer;
use crate::renderer::Renderer;
use crate::scene::RenderScene;

/// The widest or tallest image an export makes, in pixels.
pub const MAX_SIDE: u32 = 16_384;
/// The most pixels an export makes: 128 MB of RGBA, which leaves room in a
/// browser tab's memory for the copy an encoder takes.
pub const MAX_PIXELS: u64 = 32 * 1024 * 1024;

/// Pixels in rows top to bottom, four bytes each: red, green, blue and
/// straight — not premultiplied — alpha, which is what encoders and the
/// web's `ImageData` expect.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// The artboard at `scale` pixels per document unit. With `transparent`,
/// the page itself is left out and only the artwork drawn.
pub fn image(doc: &Document, scale: f64, transparent: bool) -> Result<Image, RenderError> {
    let artboard = doc.artboard();
    let (width, height) = (artboard.width * scale, artboard.height * scale);
    let (w, h) = (width.round(), height.round());
    let fits = scale.is_finite()
        && scale > 0.0
        && (1.0..=MAX_SIDE as f64).contains(&w)
        && (1.0..=MAX_SIDE as f64).contains(&h)
        && (w * h) as u64 <= MAX_PIXELS;
    if !fits {
        return Err(RenderError::ExportSize { width, height });
    }
    let (w, h) = (w as u32, h as u32);

    let mut scene = RenderScene::build(doc)?;
    scene.background = None;
    if transparent {
        scene.artboard = None;
    }
    let mut pixmap = Pixmap::new(w, h).ok_or(RenderError::BadTargetSize {
        width: w,
        height: h,
    })?;
    // Exactly onto the pixel grid, even where the scaled size was rounded.
    let view = Affine::scale_non_uniform(w as f64 / artboard.width, h as f64 / artboard.height);
    let whole = Rect::new(0.0, 0.0, w.into(), h.into());
    CpuRenderer::new().render(&scene, view, whole, &mut pixmap)?;

    let mut pixels = Vec::with_capacity(pixmap.data().len());
    for pixel in pixmap.pixels() {
        let c = pixel.demultiply();
        pixels.extend([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(Image {
        width: w,
        height: h,
        pixels,
    })
}
