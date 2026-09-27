//! graphicgene rendering: the render scene, the `Renderer` trait, and the v0.1
//! CPU backend.

pub mod cpu;
pub mod renderer;
pub mod scene;

pub use cpu::{CpuRenderer, PixelRect, scroll};
pub use renderer::Renderer;
pub use scene::{AA_MARGIN, Artboard, Damage, RenderItem, RenderScene, device_area};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error(transparent)]
    Core(#[from] graphicgene_core::error::CoreError),

    #[error("render target is {width}x{height}, which is not a valid pixmap size")]
    BadTargetSize { width: u32, height: u32 },
}
