//! Text: a node holding a string and a style, and the layout that turns
//! them into glyph outlines, drawn like any other path.
//!
//! The shaping here is deliberately simple: each character is mapped to a
//! glyph through the font's cmap, advanced by its advance width, and kerned
//! against the glyph before it — OpenType GPOS pair kerning, or the older
//! `kern` table. That sets Latin, Greek and Cyrillic text, and Chinese,
//! Japanese and Korean, the way a browser does, bar ligatures. Scripts that
//! need real shaping — Arabic joining, Indic reordering, Thai marks — do
//! not come out right. A full shaper (rustybuzz, harfrust) would replace
//! `shape_line` when they matter, at 200–300 KB of gzipped wasm against
//! this 41 KB; see ROADMAP.md.
//!
//! Lines break at newlines only, so a text box is as wide as its longest
//! line. Each line box follows CSS — the line height split evenly above the
//! font's ascent and below its descent — so the page's editing overlay, a
//! browser text field, lines up with what is drawn.

use std::collections::BTreeSet;
use std::sync::Arc;

use kurbo::{Affine, BezPath, Point, Rect};
use serde::{Deserialize, Serialize};
use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
use ttf_parser::{Face, GlyphId, OutlineBuilder, Tag};

use crate::color::LinearRgba;
use crate::fonts::Fonts;
use crate::paint::Paint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    /// A family name; the session's fonts supply its faces.
    pub family: String,
    /// In document units.
    pub size: f64,
    /// A multiple of the size.
    pub line_height: f64,
    pub align: TextAlign,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: String::new(),
            size: 24.0,
            line_height: 1.2,
            align: TextAlign::Left,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextNode {
    pub content: String,
    pub style: TextStyle,
    pub fill: Option<Paint>,
    /// Outlines and the text box, from the layout pass. Not saved: they
    /// follow from the content, the style and the fonts at hand.
    #[serde(skip)]
    pub layout: Option<Arc<TextLayout>>,
    /// What `layout` was made from, to tell when it is out of date.
    #[serde(skip)]
    pub(crate) layout_key: u64,
}

impl TextNode {
    /// Text in one colour, or none.
    pub fn new(content: impl Into<String>, style: TextStyle, fill: Option<LinearRgba>) -> Self {
        Self {
            content: content.into(),
            style,
            fill: fill.map(Paint::Solid),
            layout: None,
            layout_key: 0,
        }
    }

    /// The text box, or where it will be before the first layout: one empty
    /// line.
    pub fn bounds(&self) -> Rect {
        match &self.layout {
            Some(layout) => layout.bounds,
            None => Rect::new(0.0, 0.0, 0.0, self.style.size * self.style.line_height),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextLayout {
    /// Every glyph's outline in the node's own space: y down, the text
    /// box's top-left corner at the origin.
    pub path: BezPath,
    /// As wide as the longest line, a line height per line.
    pub bounds: Rect,
    /// Characters no font at hand has, so nothing was drawn for them. The
    /// app layer may fetch fonts that do.
    pub missing: BTreeSet<char>,
}

/// Set `content` in `style` with the fonts at hand.
pub fn layout(fonts: &Fonts, content: &str, style: &TextStyle) -> TextLayout {
    let faces = fonts.faces_for(&style.family);
    let mut shaper = Shaper {
        kern_lookups: vec![None; faces.len()],
        faces: &faces,
        own: fonts.own_faces(&style.family),
        size: style.size,
        missing: BTreeSet::new(),
        last_face: 0,
    };

    // Vertical metrics come from the family's first face — or, lacking
    // one, from whatever font there is, or a common shape of font.
    let first = faces.first();
    let (ascent, descent) = first.map_or((0.8, -0.2), |face| {
        let em = f64::from(face.units_per_em());
        (
            f64::from(face.ascender()) / em,
            f64::from(face.descender()) / em,
        )
    });
    let line_height = style.size * style.line_height;
    let baseline = (line_height - (ascent - descent) * style.size) / 2.0 + ascent * style.size;

    let lines: Vec<(BezPath, f64)> = content
        .split('\n')
        .map(|line| shaper.line(line.trim_end_matches('\r')))
        .collect();
    let width = lines.iter().map(|&(_, w)| w).fold(0.0, f64::max);
    let mut path = BezPath::new();
    for (i, (line, line_width)) in lines.iter().enumerate() {
        let x = match style.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (width - line_width) / 2.0,
            TextAlign::Right => width - line_width,
        };
        let y = i as f64 * line_height + baseline;
        path.extend(Affine::translate((x, y)) * line.clone());
    }
    TextLayout {
        path,
        bounds: Rect::new(0.0, 0.0, width, lines.len() as f64 * line_height),
        missing: shaper.missing,
    }
}

struct Shaper<'a> {
    /// The family's own faces, then every other face, as fallback.
    faces: &'a [Face<'a>],
    /// How many of `faces` are the family's own.
    own: usize,
    /// Each face's GPOS kerning lookups, found the first time it kerns.
    kern_lookups: Vec<Option<Vec<u16>>>,
    size: f64,
    missing: BTreeSet<char>,
    /// Where the previous character was found: text tends to stay in one
    /// face, and a family sliced for the web has a hundred of them.
    last_face: usize,
}

impl Shaper<'_> {
    /// One line from x = 0 on its baseline: its outlines and its advance.
    fn line(&mut self, line: &str) -> (BezPath, f64) {
        let mut path = BezPath::new();
        let mut x = 0.0;
        let mut previous: Option<(usize, GlyphId)> = None;
        for c in line.chars() {
            let Some((index, glyph)) = self.glyph(c) else {
                if !c.is_control() {
                    self.missing.insert(c);
                }
                previous = None;
                continue;
            };
            let face = &self.faces[index];
            let scale = self.size / f64::from(face.units_per_em());
            if let Some((before, left)) = previous
                && before == index
            {
                x += self.kerning(index, left, glyph) * scale;
            }
            let mut outline = Outline {
                path: &mut path,
                transform: Affine::translate((x, 0.0)) * Affine::scale_non_uniform(scale, -scale),
            };
            face.outline_glyph(glyph, &mut outline);
            x += f64::from(face.glyph_hor_advance(glyph).unwrap_or(0)) * scale;
            previous = Some((index, glyph));
        }
        (path, x)
    }

    /// The first face with a glyph for `c`: the family's own before any
    /// other.
    fn glyph(&mut self, c: char) -> Option<(usize, GlyphId)> {
        let found = |face: &Face| face.glyph_index(c).filter(|g| g.0 != 0);
        // The family's faces are slices of one font, so the one that had
        // the last character is as good as any — and likeliest. A fallback
        // face gets no such shortcut: the family's own come first.
        if self.last_face < self.own
            && let Some(glyph) = found(&self.faces[self.last_face])
        {
            return Some((self.last_face, glyph));
        }
        let (index, glyph) = self
            .faces
            .iter()
            .enumerate()
            .find_map(|(i, face)| found(face).map(|g| (i, g)))?;
        self.last_face = index;
        Some((index, glyph))
    }

    /// How far `right` moves against `left`, in font units: the GPOS `kern`
    /// feature, applied lookup by lookup as a shaper would, else the older
    /// `kern` table.
    fn kerning(&mut self, index: usize, left: GlyphId, right: GlyphId) -> f64 {
        let face = &self.faces[index];
        if let Some(gpos) = face.tables().gpos {
            let lookups = self.kern_lookups[index].get_or_insert_with(|| {
                let kern = Tag::from_bytes(b"kern");
                let mut indices: Vec<u16> = gpos
                    .features
                    .into_iter()
                    .filter(|feature| feature.tag == kern)
                    .flat_map(|feature| feature.lookup_indices)
                    .collect();
                indices.sort_unstable();
                indices.dedup();
                indices
            });
            if !lookups.is_empty() {
                let mut total = 0.0;
                for &lookup in lookups.iter() {
                    let Some(lookup) = gpos.lookups.get(lookup) else {
                        continue;
                    };
                    // Within a lookup, the first subtable that covers the
                    // pair is the one that applies.
                    for subtable in lookup.subtables.into_iter::<PositioningSubtable>() {
                        if let PositioningSubtable::Pair(pair) = subtable
                            && let Some(adjust) = pair_adjustment(pair, left, right)
                        {
                            total += f64::from(adjust);
                            break;
                        }
                    }
                }
                return total;
            }
        }
        face.tables()
            .kern
            .into_iter()
            .flat_map(|kern| kern.subtables)
            .filter(|table| table.horizontal && !table.variable && !table.has_cross_stream)
            .find_map(|table| table.glyphs_kerning(left, right))
            .map_or(0.0, f64::from)
    }
}

/// A pair adjustment's advance change for the first glyph, if it covers
/// the pair.
fn pair_adjustment(pair: PairAdjustment, left: GlyphId, right: GlyphId) -> Option<i16> {
    match pair {
        PairAdjustment::Format1 { coverage, sets } => {
            let (first, _) = sets.get(coverage.get(left)?)?.get(right)?;
            Some(first.x_advance)
        }
        PairAdjustment::Format2 {
            coverage,
            classes,
            matrix,
        } => {
            coverage.get(left)?;
            let (first, _) = matrix.get((classes.0.get(left), classes.1.get(right)))?;
            Some(first.x_advance)
        }
    }
}

/// Glyph outlines into a path: font units, y up, through `transform`.
struct Outline<'a> {
    path: &'a mut BezPath,
    transform: Affine,
}

impl Outline<'_> {
    fn point(&self, x: f32, y: f32) -> Point {
        self.transform * Point::new(f64::from(x), f64::from(y))
    }
}

impl OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.path.move_to(p);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.path.line_to(p);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.point(x1, y1), self.point(x, y));
        self.path.quad_to(c, p);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.point(x1, y1), self.point(x2, y2), self.point(x, y));
        self.path.curve_to(c1, c2, p);
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}

/// A hash of what a layout depends on, so the layout pass can skip text
/// that has not changed since.
pub(crate) fn layout_key(fonts: &Fonts, text: &TextNode) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    fonts.version().hash(&mut hasher);
    text.content.hash(&mut hasher);
    text.style.family.hash(&mut hasher);
    text.style.size.to_bits().hash(&mut hasher);
    text.style.line_height.to_bits().hash(&mut hasher);
    text.style.align.hash(&mut hasher);
    // Never 0, which means "not laid out".
    hasher.finish() | 1
}
