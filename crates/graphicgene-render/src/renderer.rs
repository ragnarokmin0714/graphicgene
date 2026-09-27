//! The renderer interface.
//!
//! `render` takes a dirty rect, and the scene's `update` reports one: only the
//! area where something changed is redrawn. The interface had that shape from
//! the start, so adding incremental redraw changed no call site.
//!
//! The GPU backend (`wgpu` / `vello`) will be the second implementation of this
//! trait. An abstraction with one implementation is not an abstraction — the
//! CPU renderer exists partly to prove this trait is honest before the desktop
//! and GPU work starts.

use graphicgene_core::geom::{Affine, Bounds};

use crate::scene::RenderScene;

pub trait Renderer {
    /// What this backend draws into — a CPU pixmap, a GPU surface texture, ...
    type Target;
    type Error;

    /// Redraw the part of `target` that `dirty` (device pixels) covers, from
    /// scratch: clear it to the scene's background, draw the artboard, and
    /// draw every item reaching into it through `view` (document to device
    /// pixels). The rest of `target` must be left untouched — that is what
    /// makes redrawing only what changed correct.
    fn render(
        &mut self,
        scene: &RenderScene,
        view: Affine,
        dirty: Bounds,
        target: &mut Self::Target,
    ) -> std::result::Result<(), Self::Error>;
}
