//! The fonts a session can set text in.
//!
//! Core does no IO: the app layer hands over sfnt bytes (TrueType or
//! OpenType), and this keeps them, grouped by family. A family may arrive
//! in pieces — web fonts are sliced by unicode range and fetched as text
//! needs them — so a family is a list of faces, and a character is set in
//! the first face of the text's family that has it, then in the first face
//! of any family that does.
//!
//! Fonts are session state, not document state: a document names families,
//! and whoever opens it supplies them, as with any design tool.

use crate::error::{CoreError, Result};

#[derive(Debug, Default)]
pub struct Fonts {
    faces: Vec<Font>,
    /// Bumped with every face added, so text laid out before it can tell.
    version: u64,
}

#[derive(Debug)]
struct Font {
    family: String,
    data: Box<[u8]>,
}

impl Fonts {
    /// Take a font's bytes. Returns its family name.
    pub fn add(&mut self, data: Vec<u8>) -> Result<String> {
        let face = ttf_parser::Face::parse(&data, 0).map_err(|_| CoreError::BadFont)?;
        let family = family_name(&face).ok_or(CoreError::BadFont)?;
        self.faces.push(Font {
            family: family.clone(),
            data: data.into_boxed_slice(),
        });
        self.version += 1;
        Ok(family)
    }

    /// Family names, in the order their first face arrived.
    pub fn families(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for font in &self.faces {
            if !out.contains(&font.family.as_str()) {
                out.push(&font.family);
            }
        }
        out
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Every face, parsed, with whether it belongs to `family`: the text's
    /// own faces first, then the rest, which characters fall back to.
    pub(crate) fn faces_for(&self, family: &str) -> Vec<ttf_parser::Face<'_>> {
        let own = self.faces.iter().filter(|f| f.family == family);
        let others = self.faces.iter().filter(|f| f.family != family);
        own.chain(others)
            .filter_map(|font| ttf_parser::Face::parse(&font.data, 0).ok())
            .collect()
    }

    /// How many of `faces_for(family)` are the family's own.
    pub(crate) fn own_faces(&self, family: &str) -> usize {
        self.faces.iter().filter(|f| f.family == family).count()
    }
}

/// The typographic family name if there is one — "Noto Sans TC" rather than
/// "Noto Sans TC Regular" — else the plain family name.
fn family_name(face: &ttf_parser::Face) -> Option<String> {
    let named = |id: u16| {
        face.names()
            .into_iter()
            .filter(|name| name.name_id == id && name.is_unicode())
            .find_map(|name| name.to_string())
    };
    named(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| named(ttf_parser::name_id::FAMILY))
}
