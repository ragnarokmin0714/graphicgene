//! The layout pass: what has to be worked out from the document before it
//! can be drawn, run at the start of every frame.
//!
//! Today that is text — each text node's glyph outlines and box, laid out
//! again when its content or style changes, or when fonts arrive. Figma-style
//! auto layout will live here too, which is why the pass was wired into the
//! pipeline (document -> layout -> RenderScene -> Renderer) before it had
//! anything to do.
//!
//! An idle frame costs nothing here: the pass looks only at nodes in the
//! document's pending change log, and walks the whole tree only when the
//! tree's shape changed or new fonts came in.

use std::sync::Arc;

use crate::doc::Document;
use crate::error::Result;
use crate::fonts::Fonts;
use crate::node::{NodeId, NodeKind};
use crate::text;

fn is_text(doc: &Document, id: NodeId) -> bool {
    matches!(doc.get(id).map(|node| &node.kind), Ok(NodeKind::Text(_)))
}

/// Lay out whatever needs it. `fonts_seen` is the fonts' version the last
/// pass ran with, kept by the caller; returns whether any text was laid
/// out, which can change what glyphs are missing.
pub fn run(doc: &mut Document, fonts: &Fonts, fonts_seen: &mut u64) -> Result<bool> {
    let changes = doc.pending_changes();
    let ids: Vec<NodeId> =
        if *fonts_seen != fonts.version() || changes.structure || changes.everything {
            doc.walk()
        } else if !changes.nodes.iter().any(|&id| is_text(doc, id)) {
            // The common case, every frame of a drag: nothing to lay out,
            // and nothing allocated to find that out.
            return Ok(false);
        } else {
            changes.nodes.iter().copied().collect()
        };
    *fonts_seen = fonts.version();

    let mut laid_out = false;
    for id in ids {
        let Ok(node) = doc.get(id) else { continue };
        let NodeKind::Text(text) = &node.kind else {
            continue;
        };
        let key = text::layout_key(fonts, text);
        if text.layout_key == key {
            continue;
        }
        let layout = Arc::new(text::layout(fonts, &text.content, &text.style));
        if let NodeKind::Text(text) = &mut doc.get_mut(id)?.kind {
            text.layout = Some(layout);
            text.layout_key = key;
            laid_out = true;
        }
    }
    Ok(laid_out)
}
