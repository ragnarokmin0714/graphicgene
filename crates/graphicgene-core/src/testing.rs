//! A font made in code, for tests: rectangular glyphs with round-number
//! metrics and whatever kerning a test asks for. Nothing to vendor, nothing
//! to license, and every number a test checks is written right here.
//!
//! 1000 units per em, ascender 800, descender -200. Each glyph is a box from
//! 50 units in to 50 short of its advance, 700 tall on the baseline.

/// Builds the sfnt bytes of a TrueType font.
#[derive(Debug, Clone)]
pub struct TestFont {
    family: String,
    glyphs: Vec<(char, u16)>,
    kerning: Vec<(char, char, i16)>,
    gpos: bool,
}

pub const UNITS_PER_EM: u16 = 1000;
pub const ASCENDER: i16 = 800;
pub const DESCENDER: i16 = -200;
/// How tall every glyph's box is, in font units.
pub const GLYPH_HEIGHT: i16 = 700;

impl TestFont {
    pub fn new(family: &str) -> Self {
        Self {
            family: family.to_owned(),
            glyphs: Vec::new(),
            kerning: Vec::new(),
            gpos: false,
        }
    }

    /// A glyph for `c`, `advance` units wide.
    pub fn glyph(mut self, c: char, advance: u16) -> Self {
        self.glyphs.push((c, advance));
        self
    }

    pub fn glyphs(mut self, chars: &str, advance: u16) -> Self {
        for c in chars.chars() {
            self.glyphs.push((c, advance));
        }
        self
    }

    /// Move `right` by `value` units when it follows `left`.
    pub fn kern(mut self, left: char, right: char, value: i16) -> Self {
        self.kerning.push((left, right, value));
        self
    }

    /// Keep the kerning in GPOS, as modern fonts do, rather than `kern`.
    pub fn gpos(mut self) -> Self {
        self.gpos = true;
        self
    }

    pub fn build(&self) -> Vec<u8> {
        let mut glyphs = self.glyphs.clone();
        glyphs.sort_by_key(|&(c, _)| c);
        glyphs.dedup_by_key(|&mut (c, _)| c);
        // Glyph 0 is .notdef, empty.
        let id = |c: char| {
            glyphs
                .iter()
                .position(|&(g, _)| g == c)
                .map(|i| i as u16 + 1)
        };
        let count = glyphs.len() as u16 + 1;

        let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
            (b"head", head()),
            (b"hhea", hhea(count)),
            (b"maxp", maxp(count)),
            (b"cmap", cmap(&glyphs)),
            (b"hmtx", hmtx(&glyphs)),
            (b"name", name(&self.family)),
        ];
        let (glyf, loca) = glyf_loca(&glyphs);
        tables.push((b"glyf", glyf));
        tables.push((b"loca", loca));
        let mut pairs: Vec<(u16, u16, i16)> = self
            .kerning
            .iter()
            .filter_map(|&(l, r, v)| Some((id(l)?, id(r)?, v)))
            .collect();
        pairs.sort();
        if !pairs.is_empty() {
            if self.gpos {
                tables.push((b"GPOS", gpos(&pairs)));
            } else {
                tables.push((b"kern", kern(&pairs)));
            }
        }
        sfnt(tables)
    }
}

fn be16(out: &mut Vec<u8>, v: u16) {
    out.extend(v.to_be_bytes());
}

fn be32(out: &mut Vec<u8>, v: u32) {
    out.extend(v.to_be_bytes());
}

/// `searchRange`, `entrySelector` and `rangeShift` for `n` items of `size`
/// bytes, as binary-searched tables want them.
fn search(out: &mut Vec<u8>, n: u16, size: u16) {
    let mut power = 1u16;
    let mut log = 0u16;
    while power * 2 <= n.max(1) {
        power *= 2;
        log += 1;
    }
    be16(out, power * size);
    be16(out, log);
    be16(out, n * size - power * size);
}

fn head() -> Vec<u8> {
    let mut t = Vec::new();
    be32(&mut t, 0x0001_0000); // version
    be32(&mut t, 0x0001_0000); // fontRevision
    be32(&mut t, 0); // checkSumAdjustment
    be32(&mut t, 0x5F0F_3CF5); // magicNumber
    be16(&mut t, 0); // flags
    be16(&mut t, UNITS_PER_EM);
    t.extend([0; 16]); // created, modified
    for v in [0i16, DESCENDER, 2000, ASCENDER] {
        be16(&mut t, v as u16); // xMin, yMin, xMax, yMax
    }
    be16(&mut t, 0); // macStyle
    be16(&mut t, 8); // lowestRecPPEM
    be16(&mut t, 2); // fontDirectionHint
    be16(&mut t, 1); // indexToLocFormat: long
    be16(&mut t, 0); // glyphDataFormat
    t
}

fn hhea(count: u16) -> Vec<u8> {
    let mut t = Vec::new();
    be32(&mut t, 0x0001_0000);
    be16(&mut t, ASCENDER as u16);
    be16(&mut t, DESCENDER as u16);
    be16(&mut t, 0); // lineGap
    be16(&mut t, 2000); // advanceWidthMax
    t.extend([0; 22]); // min side bearings, extent, caret, reserved, metricDataFormat
    be16(&mut t, count); // numberOfHMetrics
    t
}

fn maxp(count: u16) -> Vec<u8> {
    let mut t = Vec::new();
    be32(&mut t, 0x0000_5000);
    be16(&mut t, count);
    t
}

/// Format 4, one segment per character, then the closing 0xFFFF segment.
fn cmap(glyphs: &[(char, u16)]) -> Vec<u8> {
    let mut segments: Vec<(u16, u16)> = glyphs
        .iter()
        .enumerate()
        .map(|(i, &(c, _))| (c as u16, (i + 1) as u16))
        .collect();
    segments.push((0xFFFF, 0));
    let n = segments.len() as u16;
    let mut sub = Vec::new();
    be16(&mut sub, 4);
    be16(&mut sub, 16 + 8 * n); // length
    be16(&mut sub, 0); // language
    be16(&mut sub, n * 2);
    search(&mut sub, n, 2);
    for &(code, _) in &segments {
        be16(&mut sub, code); // endCode
    }
    be16(&mut sub, 0); // reservedPad
    for &(code, _) in &segments {
        be16(&mut sub, code); // startCode
    }
    for &(code, glyph) in &segments {
        let delta = if code == 0xFFFF {
            1
        } else {
            glyph.wrapping_sub(code)
        };
        be16(&mut sub, delta); // idDelta
    }
    for _ in &segments {
        be16(&mut sub, 0); // idRangeOffset
    }
    let mut t = Vec::new();
    be16(&mut t, 0); // version
    be16(&mut t, 1); // numTables
    be16(&mut t, 3); // Windows
    be16(&mut t, 1); // Unicode BMP
    be32(&mut t, 12);
    t.extend(sub);
    t
}

fn hmtx(glyphs: &[(char, u16)]) -> Vec<u8> {
    let mut t = Vec::new();
    be16(&mut t, 500); // .notdef
    be16(&mut t, 0);
    for &(_, advance) in glyphs {
        be16(&mut t, advance);
        be16(&mut t, 50); // left side bearing
    }
    t
}

fn name(family: &str) -> Vec<u8> {
    let text: Vec<u8> = family.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut t = Vec::new();
    be16(&mut t, 0); // format
    be16(&mut t, 1); // count
    be16(&mut t, 6 + 12); // stringOffset
    for v in [3, 1, 0x0409, 1] {
        be16(&mut t, v); // platform, encoding, language, name id: family
    }
    be16(&mut t, text.len() as u16);
    be16(&mut t, 0);
    t.extend(text);
    t
}

/// A box per glyph, on the baseline, 50 units in from each side.
fn glyf_loca(glyphs: &[(char, u16)]) -> (Vec<u8>, Vec<u8>) {
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    be32(&mut loca, 0);
    be32(&mut loca, 0); // .notdef is empty
    for &(_, advance) in glyphs {
        let (x0, x1, y1) = (50i16, advance as i16 - 50, GLYPH_HEIGHT);
        be16(&mut glyf, 1); // numberOfContours
        for v in [x0, 0, x1, y1] {
            be16(&mut glyf, v as u16);
        }
        be16(&mut glyf, 3); // endPtsOfContours
        be16(&mut glyf, 0); // instructionLength
        glyf.extend([1u8; 4]); // on curve, two-byte deltas
        for dx in [x0, x1 - x0, 0, x0 - x1] {
            be16(&mut glyf, dx as u16);
        }
        for dy in [0, 0, y1, 0] {
            be16(&mut glyf, dy as u16);
        }
        be32(&mut loca, glyf.len() as u32);
    }
    (glyf, loca)
}

/// The Windows `kern` table: version 0, one horizontal format 0 subtable.
fn kern(pairs: &[(u16, u16, i16)]) -> Vec<u8> {
    let n = pairs.len() as u16;
    let mut t = Vec::new();
    be16(&mut t, 0); // version
    be16(&mut t, 1); // nTables
    be16(&mut t, 0); // subtable version
    be16(&mut t, 14 + 6 * n); // length
    be16(&mut t, 0x0001); // coverage: horizontal, format 0
    be16(&mut t, n);
    search(&mut t, n, 6);
    for &(left, right, value) in pairs {
        be16(&mut t, left);
        be16(&mut t, right);
        be16(&mut t, value as u16);
    }
    t
}

/// GPOS with one `kern` feature: a pair adjustment lookup, format 1.
fn gpos(pairs: &[(u16, u16, i16)]) -> Vec<u8> {
    let mut lefts: Vec<u16> = pairs.iter().map(|&(l, _, _)| l).collect();
    lefts.dedup();

    // PairPosFormat1, then its coverage, then its pair sets.
    let head_len = 10 + 2 * lefts.len();
    let coverage_len = 4 + 2 * lefts.len();
    let mut pair_pos = Vec::new();
    be16(&mut pair_pos, 1); // posFormat
    be16(&mut pair_pos, head_len as u16); // coverageOffset
    be16(&mut pair_pos, 0x0004); // valueFormat1: XAdvance
    be16(&mut pair_pos, 0); // valueFormat2
    be16(&mut pair_pos, lefts.len() as u16);
    let mut sets = Vec::new();
    for &left in &lefts {
        be16(&mut pair_pos, (head_len + coverage_len + sets.len()) as u16);
        let set: Vec<_> = pairs.iter().filter(|&&(l, _, _)| l == left).collect();
        be16(&mut sets, set.len() as u16);
        for &&(_, right, value) in &set {
            be16(&mut sets, right);
            be16(&mut sets, value as u16);
        }
    }
    be16(&mut pair_pos, 1); // coverageFormat
    be16(&mut pair_pos, lefts.len() as u16);
    for &left in &lefts {
        be16(&mut pair_pos, left);
    }
    pair_pos.extend(sets);

    let mut lookup_list = Vec::new();
    be16(&mut lookup_list, 1); // lookupCount
    be16(&mut lookup_list, 4); // lookup offset
    be16(&mut lookup_list, 2); // lookupType: pair adjustment
    be16(&mut lookup_list, 0); // lookupFlag
    be16(&mut lookup_list, 1); // subTableCount
    be16(&mut lookup_list, 8); // subtable offset, from the lookup
    lookup_list.extend(pair_pos);

    let mut feature_list = Vec::new();
    be16(&mut feature_list, 1); // featureCount
    feature_list.extend(b"kern");
    be16(&mut feature_list, 8); // feature offset
    be16(&mut feature_list, 0); // featureParamsOffset
    be16(&mut feature_list, 1); // lookupIndexCount
    be16(&mut feature_list, 0);

    let mut script_list = Vec::new();
    be16(&mut script_list, 1); // scriptCount
    script_list.extend(b"DFLT");
    be16(&mut script_list, 8); // script offset
    be16(&mut script_list, 4); // defaultLangSysOffset, from the script
    be16(&mut script_list, 0); // langSysCount
    be16(&mut script_list, 0); // lookupOrderOffset
    be16(&mut script_list, 0xFFFF); // requiredFeatureIndex
    be16(&mut script_list, 1); // featureIndexCount
    be16(&mut script_list, 0);

    let mut t = Vec::new();
    be16(&mut t, 1);
    be16(&mut t, 0);
    let scripts = 10;
    let features = scripts + script_list.len();
    let lookups = features + feature_list.len();
    be16(&mut t, scripts as u16);
    be16(&mut t, features as u16);
    be16(&mut t, lookups as u16);
    t.extend(script_list);
    t.extend(feature_list);
    t.extend(lookup_list);
    t
}

/// The sfnt container: the table directory sorted by tag, each table padded
/// to four bytes. Checksums are left at zero; nothing here checks them.
fn sfnt(mut tables: Vec<(&[u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|&(tag, _)| *tag);
    let n = tables.len() as u16;
    let mut out = Vec::new();
    be32(&mut out, 0x0001_0000);
    be16(&mut out, n);
    search(&mut out, n, 16);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend(*tag);
        be32(&mut out, 0);
        be32(&mut out, offset as u32);
        be32(&mut out, data.len() as u32);
        body.extend(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * tables.len() + body.len();
    }
    out.extend(body);
    out
}
