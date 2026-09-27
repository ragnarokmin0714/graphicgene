//! The view: how the document is laid onto the screen — zoom and pan.
//!
//! Per-viewer state, not document state: it is never saved and never part
//! of undo. It lives in core anyway so that every shell zooms, pans and fits
//! the same way, and so the maths is tested once.
//!
//! Three spaces meet here. *Document* units are what the artwork is drawn in.
//! *Screen* pixels are CSS pixels from the viewport's top-left corner — what
//! pointer events report. *Device* pixels are the canvas's own pixels:
//! screen pixels times the device pixel ratio.
//!
//! The pan is kept on whole device pixels. That keeps an unchanged picture
//! reusable when panning — it moves by a whole number of pixels, so the
//! renderer can shift what it has and draw only the strips that appear —
//! and it keeps edges that sit on whole document units crisp at 100%.

use crate::geom::{Affine, Point, Rect, Size, Vec2};

/// Figma's range: small enough to see a whole large board, large enough to
/// place points at sub-pixel precision.
pub const MIN_ZOOM: f64 = 0.02;
pub const MAX_ZOOM: f64 = 256.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Screen pixels per document unit.
    zoom: f64,
    /// Where the document origin is, in screen pixels.
    pan: Vec2,
    /// The viewport in device pixels.
    device: (u32, u32),
    /// Device pixels per screen pixel.
    dpr: f64,
}

impl View {
    /// A view at 100% with the document origin at the viewport's corner.
    pub fn new(device_width: u32, device_height: u32, dpr: f64) -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
            device: (device_width.max(1), device_height.max(1)),
            dpr: sane_dpr(dpr),
        }
    }

    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    /// Where the document origin is, in screen pixels.
    pub fn pan(&self) -> Vec2 {
        self.pan
    }

    pub fn dpr(&self) -> f64 {
        self.dpr
    }

    /// The viewport in device pixels: the size the canvas's pixels must be.
    pub fn device_size(&self) -> (u32, u32) {
        self.device
    }

    /// The viewport in screen pixels.
    pub fn screen_size(&self) -> Size {
        Size::new(
            f64::from(self.device.0) / self.dpr,
            f64::from(self.device.1) / self.dpr,
        )
    }

    /// Document to screen pixels.
    pub fn to_screen(&self) -> Affine {
        Affine::translate(self.pan) * Affine::scale(self.zoom)
    }

    /// Document to device pixels: what the renderer draws with.
    pub fn to_device(&self) -> Affine {
        Affine::scale(self.dpr) * self.to_screen()
    }

    pub fn screen_to_document(&self, point: Point) -> Point {
        ((point.to_vec2() - self.pan) / self.zoom).to_point()
    }

    /// A screen distance — a pick tolerance, say — in document units.
    pub fn screen_distance_to_document(&self, distance: f64) -> f64 {
        distance / self.zoom
    }

    /// The viewport changed size, or moved to a screen with another pixel
    /// ratio. The document stays where it was on screen.
    pub fn resize(&mut self, device_width: u32, device_height: u32, dpr: f64) {
        self.device = (device_width.max(1), device_height.max(1));
        self.dpr = sane_dpr(dpr);
        self.snap();
    }

    /// Move the picture by a screen-space offset.
    pub fn pan_by(&mut self, delta: Vec2) {
        self.pan += delta;
        self.snap();
    }

    /// Set the zoom, keeping the document point under `anchor` (a screen
    /// point, usually the pointer) where it is.
    pub fn zoom_to(&mut self, zoom: f64, anchor: Point) {
        let fixed = self.screen_to_document(anchor);
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = anchor.to_vec2() - fixed.to_vec2() * self.zoom;
        self.snap();
    }

    /// Multiply the zoom by `factor` around `anchor`: a wheel or pinch step.
    pub fn zoom_by(&mut self, factor: f64, anchor: Point) {
        if factor.is_finite() && factor > 0.0 {
            self.zoom_to(self.zoom * factor, anchor);
        }
    }

    /// The next power of two up — 25%, 50%, 100%, 200% — around `anchor`.
    pub fn zoom_in(&mut self, anchor: Point) {
        let step = (self.zoom.log2() + 1e-9).floor() + 1.0;
        self.zoom_to(step.exp2(), anchor);
    }

    /// The next power of two down, around `anchor`.
    pub fn zoom_out(&mut self, anchor: Point) {
        let step = (self.zoom.log2() - 1e-9).ceil() - 1.0;
        self.zoom_to(step.exp2(), anchor);
    }

    /// Show all of `area` (document space), centred, with `padding` screen
    /// pixels around it, never zooming in past `max_zoom`.
    pub fn fit(&mut self, area: Rect, padding: f64, max_zoom: f64) {
        let screen = self.screen_size();
        let room = Size::new(
            (screen.width - 2.0 * padding).max(1.0),
            (screen.height - 2.0 * padding).max(1.0),
        );
        let fits = (room.width / area.width().max(1e-9)).min(room.height / area.height().max(1e-9));
        self.zoom = fits.min(max_zoom).clamp(MIN_ZOOM, MAX_ZOOM);
        let centre = Vec2::new(screen.width / 2.0, screen.height / 2.0);
        self.pan = centre - area.center().to_vec2() * self.zoom;
        self.snap();
    }

    /// Set zoom and pan directly, as when restoring a saved view.
    pub fn set(&mut self, zoom: f64, pan: Vec2) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = pan;
        self.snap();
    }

    /// How far, in whole device pixels, a picture drawn in `earlier` has to
    /// move to be this view's picture — or `None` when shifting pixels
    /// cannot produce it (a different zoom, size or pixel ratio).
    pub fn scroll_from(&self, earlier: &View) -> Option<(i32, i32)> {
        if self.zoom != earlier.zoom || self.dpr != earlier.dpr || self.device != earlier.device {
            return None;
        }
        let shift = (self.pan - earlier.pan) * self.dpr;
        let (dx, dy) = (shift.x.round(), shift.y.round());
        // Both pans are snapped, so this only fails if something bypassed it.
        if (shift.x - dx).abs() > 1e-6 || (shift.y - dy).abs() > 1e-6 {
            return None;
        }
        Some((dx as i32, dy as i32))
    }

    /// Put the pan on whole device pixels.
    fn snap(&mut self) {
        self.pan = Vec2::new(
            (self.pan.x * self.dpr).round() / self.dpr,
            (self.pan.y * self.dpr).round() / self.dpr,
        );
    }
}

fn sane_dpr(dpr: f64) -> f64 {
    if dpr.is_finite() && dpr > 0.0 {
        dpr
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point, tolerance: f64) -> bool {
        (a - b).hypot() <= tolerance
    }

    #[test]
    fn screen_and_document_round_trip() {
        let mut view = View::new(1000, 800, 2.0);
        view.set(1.5, Vec2::new(100.0, -40.0));
        let doc = Point::new(12.25, 80.5);
        let screen = view.to_screen() * doc;
        assert!(close(view.screen_to_document(screen), doc, 1e-9));
        // Device pixels are screen pixels times the ratio.
        assert!(close(
            view.to_device() * doc,
            (screen.to_vec2() * 2.0).to_point(),
            1e-9
        ));
        assert_eq!(view.screen_distance_to_document(6.0), 4.0);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_still() {
        let mut view = View::new(1000, 800, 1.0);
        let pointer = Point::new(300.0, 200.0);
        let under = view.screen_to_document(pointer);
        view.zoom_by(1.7, pointer);
        // Snapping the pan may move it by less than a device pixel.
        assert!(close(view.to_screen() * under, pointer, 0.5));
        assert!((view.zoom() - 1.7).abs() < 1e-12);
    }

    #[test]
    fn zoom_is_clamped_and_steps_by_powers_of_two() {
        let mut view = View::new(100, 100, 1.0);
        let centre = Point::new(50.0, 50.0);
        view.zoom_by(1e6, centre);
        assert_eq!(view.zoom(), MAX_ZOOM);
        view.zoom_by(1e-9, centre);
        assert_eq!(view.zoom(), MIN_ZOOM);

        view.set(1.0, Vec2::ZERO);
        view.zoom_in(centre);
        assert_eq!(view.zoom(), 2.0);
        view.zoom_out(centre);
        view.zoom_out(centre);
        assert_eq!(view.zoom(), 0.5);
        // From between steps, to the next one.
        view.set(0.7, Vec2::ZERO);
        view.zoom_in(centre);
        assert_eq!(view.zoom(), 1.0);
        view.set(0.7, Vec2::ZERO);
        view.zoom_out(centre);
        assert_eq!(view.zoom(), 0.5);
    }

    #[test]
    fn fitting_centres_the_area_within_the_padding() {
        let mut view = View::new(1000, 700, 1.0);
        let board = Rect::new(0.0, 0.0, 800.0, 600.0);
        // Fits at 100%, and 100% is the cap: centred, unscaled.
        view.fit(board, 40.0, 1.0);
        assert_eq!(view.zoom(), 1.0);
        assert_eq!(view.pan(), Vec2::new(100.0, 50.0));

        // Too big: scaled down to the room left inside the padding.
        view.fit(Rect::new(0.0, 0.0, 1840.0, 620.0), 40.0, 1.0);
        assert!((view.zoom() - 0.5).abs() < 1e-12);
        let top_left = view.to_screen() * Point::new(0.0, 0.0);
        assert!((top_left.x - 40.0).abs() <= 0.5);
    }

    #[test]
    fn the_pan_stays_on_whole_device_pixels() {
        let mut view = View::new(900, 600, 1.5);
        view.pan_by(Vec2::new(10.3, 7.7));
        let device = view.pan() * view.dpr();
        assert_eq!(device.x, device.x.round());
        assert_eq!(device.y, device.y.round());
        view.zoom_by(1.23, Point::new(123.4, 56.7));
        let device = view.pan() * view.dpr();
        assert_eq!(device.x, device.x.round());
    }

    #[test]
    fn panning_can_reuse_pixels_and_zooming_cannot() {
        let mut view = View::new(800, 600, 2.0);
        let before = view;
        view.pan_by(Vec2::new(10.0, -5.0));
        assert_eq!(view.scroll_from(&before), Some((20, -10)));
        let panned = view;
        view.zoom_by(2.0, Point::new(0.0, 0.0));
        assert_eq!(view.scroll_from(&panned), None);
        let mut resized = panned;
        resized.resize(810, 600, 2.0);
        assert_eq!(resized.scroll_from(&panned), None);
    }

    #[test]
    fn nonsense_ratios_and_sizes_are_tamed() {
        let view = View::new(0, 0, f64::NAN);
        assert_eq!(view.device_size(), (1, 1));
        assert_eq!(view.dpr(), 1.0);
    }
}
