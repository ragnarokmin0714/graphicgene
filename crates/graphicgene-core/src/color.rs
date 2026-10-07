//! Colour representation.
//!
//! Colours are stored as `f32` RGBA in **linear sRGB**, never as 8-bit sRGB.
//! This is a load-bearing decision, not a preference: 16-bit depth, wide gamut
//! and correct blending all require it, and all three become unreachable once
//! 8-bit sRGB is baked into the document model. See CLAUDE.md.

use serde::{Deserialize, Serialize};

/// Linear-sRGB colour with straight (non-premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LinearRgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl LinearRgba {
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Convert from the 8-bit sRGB values that UI colour pickers and SVG use.
    pub fn from_srgb8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: srgb_to_linear(r as f32 / 255.0),
            g: srgb_to_linear(g as f32 / 255.0),
            b: srgb_to_linear(b as f32 / 255.0),
            a: a as f32 / 255.0,
        }
    }

    /// Convert back to 8-bit sRGB for display, export and pickers.
    ///
    /// Lossy by definition — never round-trip document state through this.
    pub fn to_srgb8(self) -> [u8; 4] {
        [
            encode(linear_to_srgb(self.r)),
            encode(linear_to_srgb(self.g)),
            encode(linear_to_srgb(self.b)),
            encode(self.a),
        ]
    }
}

/// `a` and `b` mixed, `t` of the way to `b`, in sRGB-encoded values with
/// straight alpha — the space gradients are drawn in, by the renderer and in
/// SVG — so a colour found this way matches what a gradient shows there.
pub fn mix_srgb(a: LinearRgba, b: LinearRgba, t: f32) -> LinearRgba {
    let channel = |x: f32, y: f32| {
        let (x, y) = (linear_to_srgb(x), linear_to_srgb(y));
        srgb_to_linear(x + (y - x) * t)
    };
    LinearRgba {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: a.a + (b.a - a.a) * t,
    }
}

fn encode(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_448_237 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_round_trip_is_stable_at_8_bit() {
        for v in [0u8, 1, 17, 128, 200, 254, 255] {
            let c = LinearRgba::from_srgb8(v, v, v, 255);
            assert_eq!(c.to_srgb8()[0], v, "round trip failed for {v}");
        }
    }

    #[test]
    fn mid_grey_is_not_stored_as_half() {
        // The point of linear storage: sRGB 128 is ~0.216 in linear light.
        let c = LinearRgba::from_srgb8(128, 128, 128, 255);
        assert!((c.r - 0.2158).abs() < 0.001, "got {}", c.r);
    }
}
