//! The renderer interface.
//!
//! `render` takes a dirty rect. Redrawing everything is the v0.1
//! *implementation*; it is deliberately not the v0.1 *interface*, so that
//! incremental redraw can be added without every call site changing.
//!
//! The GPU backend (`wgpu` / `vello`) will be the second implementation of this
//! trait. An abstraction with one implementation is not an abstraction — the
//! CPU renderer exists partly to prove this trait is honest before the desktop
//! and GPU work starts.

use graphicgene_core::geom::Bounds;

use crate::scene::RenderScene;

pub trait Renderer {
    /// What this backend draws into — a CPU pixmap, a GPU surface texture, ...
    type Target;
    type Error;

    fn render(
        &mut self,
        scene: &RenderScene,
        dirty: Bounds,
        target: &mut Self::Target,
    ) -> std::result::Result<(), Self::Error>;
}
