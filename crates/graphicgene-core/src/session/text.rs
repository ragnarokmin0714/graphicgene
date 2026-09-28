//! Fonts, and editing text.
//!
//! Text is typed into the platform's own text field — a DOM textarea on the
//! web — because that is where input methods for Chinese, Japanese and the
//! rest live. The shell sends what the field holds on every change, and
//! the canvas draws it through the session's layout; committing records the
//! whole edit as one undo step, as a drag does.

use std::collections::{BTreeMap, BTreeSet};

use crate::color::LinearRgba;
use crate::command::Command;
use crate::error::{CoreError, Result};
use crate::geom::{Affine, Point, Rect};
use crate::node::{Node, NodeId, NodeKind};
use crate::text::{TextNode, TextStyle};

use super::Session;

/// A layer named after its text keeps at most this many characters of it.
const NAME_LENGTH: usize = 40;

/// Text being typed.
#[derive(Debug, Clone)]
pub(super) struct TextEdit {
    id: NodeId,
    /// Made for this edit: in the tree but not the journal until it is
    /// committed with something in it.
    created: bool,
    /// What it said before.
    original: String,
}

/// The text being typed, for the shell's text field. Document space.
#[derive(Debug, Clone)]
pub struct TextView {
    pub id: NodeId,
    /// The text box's space to the document: where the field goes.
    pub transform: Affine,
    pub content: String,
    pub style: TextStyle,
    /// The text box, in its own space.
    pub bounds: Rect,
}

impl Session {
    /// Take a font's bytes — TrueType or OpenType. Returns its family.
    pub fn add_font(&mut self, data: Vec<u8>) -> Result<String> {
        self.fonts.add(data)
    }

    /// Families in the order they arrived; the first sets new text.
    pub fn font_families(&self) -> Vec<String> {
        self.fonts
            .families()
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    /// Bumped whenever text is laid out again, which may change what is
    /// missing: check `missing_glyphs` when it moves.
    pub fn glyphs_version(&self) -> u64 {
        self.glyphs_version
    }

    /// Characters the document's text uses that no font at hand has, by
    /// the family they were wanted in: what the app layer could fetch.
    pub fn missing_glyphs(&self) -> Result<BTreeMap<String, BTreeSet<char>>> {
        let mut missing: BTreeMap<String, BTreeSet<char>> = BTreeMap::new();
        for id in self.document.walk() {
            if let NodeKind::Text(text) = &self.document.get(id)?.kind
                && let Some(layout) = &text.layout
                && !layout.missing.is_empty()
            {
                let wanted = missing.entry(text.style.family.clone()).or_default();
                wanted.extend(layout.missing.iter().copied());
            }
        }
        Ok(missing)
    }

    /// Start typing new text with its box's top-left corner at `point`, in
    /// the first family loaded. The node is drawn as it is typed but joins
    /// the journal only when committed with something in it.
    pub fn begin_text(&mut self, point: Point, fill: LinearRgba) -> Result<NodeId> {
        self.end_interaction()?;
        let style = TextStyle {
            family: self
                .fonts
                .families()
                .first()
                .map_or_else(String::new, |f| (*f).to_owned()),
            ..TextStyle::default()
        };
        let mut node = Node::text("Text", TextNode::new("", style, Some(fill)));
        node.common.transform = Affine::translate(point.to_vec2());
        let root = self.document.root();
        let index = self.document.children_of(root)?.len();
        let id = self.document.insert_detached(node);
        self.document.attach(id, root, index)?;
        self.selection.set([id]);
        self.hover = None;
        self.text_edit = Some(TextEdit {
            id,
            created: true,
            original: String::new(),
        });
        Ok(id)
    }

    /// Start typing into existing text. False for anything else, or text
    /// that is locked.
    pub fn edit_text(&mut self, id: NodeId) -> Result<bool> {
        self.end_interaction()?;
        let node = self.document.get(id)?;
        let NodeKind::Text(text) = &node.kind else {
            return Ok(false);
        };
        if node.common.locked {
            return Ok(false);
        }
        self.text_edit = Some(TextEdit {
            id,
            created: false,
            original: text.content.clone(),
        });
        self.selection.set([id]);
        self.hover = None;
        Ok(true)
    }

    /// What the text field holds now. Drawn, not recorded.
    pub fn preview_text(&mut self, content: &str) -> Result<bool> {
        let Some(edit) = &self.text_edit else {
            return Ok(false);
        };
        let id = edit.id;
        if let NodeKind::Text(text) = &mut self.document.get_mut(id)?.kind {
            content.clone_into(&mut text.content);
        }
        Ok(true)
    }

    /// Record the edit as one undo step. New text left empty goes away, as
    /// does existing text emptied; a layer named after its text follows it.
    /// Returns the node, if it is still there.
    pub fn commit_text(&mut self) -> Result<Option<NodeId>> {
        let Some(edit) = self.text_edit.take() else {
            return Ok(None);
        };
        let id = edit.id;
        let content = self.text_content(id)?;
        let blank = content.trim().is_empty();

        if edit.created {
            let (parent, index) = self.document.detach(id)?;
            if blank {
                self.selection.retain_attached(&self.document);
                return Ok(None);
            }
            self.document.get_mut(id)?.common.name = layer_name(&content);
            self.journal
                .execute(&mut self.document, Command::Attach { id, parent, index })?;
            return Ok(Some(id));
        }

        if content == edit.original {
            return Ok(Some(id));
        }
        self.set_text_content(id, &edit.original)?;
        if blank {
            self.journal
                .execute(&mut self.document, Command::Detach { id })?;
            self.selection.retain_attached(&self.document);
            return Ok(None);
        }
        let mut commands = vec![Command::SetText {
            id,
            content: content.clone(),
        }];
        if self.document.get(id)?.common.name == layer_name(&edit.original) {
            let name = layer_name(&content);
            commands.push(Command::Rename { id, name });
        }
        self.journal
            .execute(&mut self.document, Command::Batch(commands))?;
        Ok(Some(id))
    }

    /// Abandon the edit: new text goes away, existing text says what it
    /// said before.
    pub fn cancel_text(&mut self) -> Result<bool> {
        let Some(edit) = self.text_edit.take() else {
            return Ok(false);
        };
        if edit.created {
            self.document.detach(edit.id)?;
            self.selection.retain_attached(&self.document);
        } else {
            self.set_text_content(edit.id, &edit.original)?;
        }
        Ok(true)
    }

    pub(super) fn text_view(&self) -> Result<Option<TextView>> {
        let Some(edit) = &self.text_edit else {
            return Ok(None);
        };
        let NodeKind::Text(text) = &self.document.get(edit.id)?.kind else {
            return Ok(None);
        };
        Ok(Some(TextView {
            id: edit.id,
            transform: self.document.world_transform(edit.id)?,
            content: text.content.clone(),
            style: text.style.clone(),
            bounds: text.bounds(),
        }))
    }

    pub(super) fn editing_text(&self) -> bool {
        self.text_edit.is_some()
    }

    fn text_content(&self, id: NodeId) -> Result<String> {
        match &self.document.get(id)?.kind {
            NodeKind::Text(text) => Ok(text.content.clone()),
            _ => Err(CoreError::NotText(id)),
        }
    }

    fn set_text_content(&mut self, id: NodeId, content: &str) -> Result<()> {
        if let NodeKind::Text(text) = &mut self.document.get_mut(id)?.kind {
            content.clone_into(&mut text.content);
        }
        Ok(())
    }
}

/// A text layer's name: its first line, cut short.
fn layer_name(content: &str) -> String {
    let line = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("Text")
        .trim();
    let mut name: String = line.chars().take(NAME_LENGTH).collect();
    if line.chars().count() > NAME_LENGTH {
        name.push('…');
    }
    name
}
