//! SVG export.
//!
//! Core does no IO: this turns the document into SVG text, and the app layer
//! decides where it goes (a download on the web, a file on the desktop).
//!
//! The tree maps one to one — groups become `<g>`, vector nodes `<path>` —
//! and each node's transform, opacity and blend mode are written as its own
//! attributes rather than baked into coordinates, so the file stays editable
//! in other tools. Hidden nodes are left out. The file is the size of the
//! document's artboard. There is no background: the artboard's white is the
//! editor's backdrop, not part of the artwork.

use std::fmt::Write;

use crate::color::LinearRgba;
use crate::doc::Document;
use crate::error::Result;
use crate::geom::Affine;
use crate::node::{BlendMode, LineCap, LineJoin, NodeId, NodeKind};

/// The document as an SVG file the size of its artboard.
pub fn to_svg(doc: &Document) -> Result<String> {
    let artboard = doc.artboard();
    let (w, h) = (number(artboard.width), number(artboard.height));
    let mut out = String::new();
    // Writing to a String cannot fail; the results below are ignored for that reason.
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#
    );
    for &child in doc.children_of(doc.root())? {
        write_node(doc, child, 1, &mut out)?;
    }
    out.push_str("</svg>\n");
    Ok(out)
}

fn write_node(doc: &Document, id: NodeId, depth: usize, out: &mut String) -> Result<()> {
    let node = doc.get(id)?;
    if !node.common.visible {
        return Ok(());
    }
    let indent = "  ".repeat(depth);

    let mut attrs = String::new();
    if !node.common.name.is_empty() {
        let _ = write!(attrs, r#" data-name="{}""#, escape(&node.common.name));
    }
    if node.common.transform != Affine::IDENTITY {
        let [a, b, c, d, e, f] = node.common.transform.as_coeffs().map(number);
        let _ = write!(attrs, r#" transform="matrix({a} {b} {c} {d} {e} {f})""#);
    }
    if node.common.opacity < 1.0 {
        let _ = write!(
            attrs,
            r#" opacity="{}""#,
            number(node.common.opacity.into())
        );
    }
    if let Some(blend) = blend_css(node.common.blend_mode) {
        let _ = write!(attrs, r#" style="mix-blend-mode:{blend}""#);
    }

    match &node.kind {
        NodeKind::Group(group) => {
            let _ = writeln!(out, "{indent}<g{attrs}>");
            for &child in &group.children {
                write_node(doc, child, depth + 1, out)?;
            }
            let _ = writeln!(out, "{indent}</g>");
        }
        // Text goes out as its outlines: the SVG then looks the same on a
        // machine without the font.
        NodeKind::Text(text) => {
            let Some(layout) = &text.layout else {
                return Ok(());
            };
            let d = layout.path.to_svg();
            if d.is_empty() {
                return Ok(());
            }
            let fill = match text.fill {
                Some(fill) => {
                    let (hex, alpha) = srgb(fill);
                    let opacity =
                        alpha.map_or(String::new(), |a| format!(r#" fill-opacity="{a}""#));
                    format!(r#" fill="{hex}"{opacity}"#)
                }
                None => r#" fill="none""#.to_owned(),
            };
            let label = escape(&text.content);
            let _ = writeln!(
                out,
                r#"{indent}<path{attrs} aria-label="{label}" d="{d}"{fill}/>"#
            );
        }
        NodeKind::Vector(vector) => {
            let d = vector.path.to_svg();
            if d.is_empty() {
                return Ok(());
            }
            let mut paint = String::new();
            match vector.fill {
                Some(fill) => {
                    let (hex, alpha) = srgb(fill);
                    let _ = write!(paint, r#" fill="{hex}""#);
                    if let Some(alpha) = alpha {
                        let _ = write!(paint, r#" fill-opacity="{alpha}""#);
                    }
                }
                None => paint.push_str(r#" fill="none""#),
            }
            if let Some(stroke) = vector.stroke {
                let (hex, alpha) = srgb(stroke.color);
                let _ = write!(
                    paint,
                    r#" stroke="{hex}" stroke-width="{}""#,
                    number(stroke.width)
                );
                if let Some(alpha) = alpha {
                    let _ = write!(paint, r#" stroke-opacity="{alpha}""#);
                }
                // SVG's defaults are butt caps, miter joins and a miter
                // limit of 4 — the renderer's too — so only differences go out.
                match stroke.cap {
                    LineCap::Butt => {}
                    LineCap::Round => paint.push_str(r#" stroke-linecap="round""#),
                    LineCap::Square => paint.push_str(r#" stroke-linecap="square""#),
                }
                match stroke.join {
                    LineJoin::Miter => {}
                    LineJoin::Round => paint.push_str(r#" stroke-linejoin="round""#),
                    LineJoin::Bevel => paint.push_str(r#" stroke-linejoin="bevel""#),
                }
                if let Some(dash) = stroke.dash {
                    let _ = write!(
                        paint,
                        r#" stroke-dasharray="{} {}""#,
                        number(dash.length),
                        number(dash.gap)
                    );
                }
            }
            let _ = writeln!(out, r#"{indent}<path{attrs} d="{d}"{paint}/>"#);
        }
    }
    Ok(())
}

/// A colour as SVG wants it: 8-bit sRGB hex, plus alpha when not opaque.
/// Lossy, as every export to 8-bit is; the document keeps linear f32.
fn srgb(color: LinearRgba) -> (String, Option<String>) {
    let [r, g, b, a] = color.to_srgb8();
    let alpha = (a < 255).then(|| number(f64::from(a) / 255.0));
    (format!("#{r:02x}{g:02x}{b:02x}"), alpha)
}

fn blend_css(mode: BlendMode) -> Option<&'static str> {
    match mode {
        BlendMode::Normal => None,
        BlendMode::Multiply => Some("multiply"),
        BlendMode::Screen => Some("screen"),
        BlendMode::Overlay => Some("overlay"),
    }
}

/// A number with at most four decimals and no trailing zeros, and never "-0".
fn number(v: f64) -> String {
    let rounded = (v * 1e4).round() / 1e4;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    format!("{rounded}")
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}
