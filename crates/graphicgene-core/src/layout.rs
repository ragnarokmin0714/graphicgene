//! The layout pass.
//!
//! A no-op in v0.1. It exists so that Figma-style auto layout has a place to
//! live that is already wired into the pipeline
//! (document -> layout -> RenderScene -> Renderer), rather than being bolted
//! on later by restructuring the pipeline.
//!
//! When auto layout arrives, this is where constraint solving and child
//! placement happen, writing resolved transforms back onto the nodes before
//! the render scene is built.

use crate::doc::Document;
use crate::error::Result;

pub fn run(_doc: &mut Document) -> Result<()> {
    Ok(())
}
