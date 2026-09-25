//! Which nodes the user is working on.
//!
//! Selection is editor-session state, not document state: it is not saved and
//! not part of undo. It still lives in core rather than in the UI, because it
//! drives transforms, and the desktop app needs the same rules as the web one.

use crate::doc::Document;
use crate::node::NodeId;

/// An ordered set of node ids, in the order they were selected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    ids: Vec<NodeId>,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ids(&self) -> &[NodeId] {
        &self.ids
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.ids.contains(&id)
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn clear(&mut self) {
        self.ids.clear();
    }

    /// Replace the selection. Duplicates are dropped, first occurrence wins.
    pub fn set(&mut self, ids: impl IntoIterator<Item = NodeId>) {
        self.ids.clear();
        for id in ids {
            self.add(id);
        }
    }

    pub fn add(&mut self, id: NodeId) {
        if !self.contains(id) {
            self.ids.push(id);
        }
    }

    pub fn toggle(&mut self, id: NodeId) {
        match self.ids.iter().position(|&s| s == id) {
            Some(index) => {
                self.ids.remove(index);
            }
            None => self.ids.push(id),
        }
    }

    /// Apply a click that landed on `hit` (or on nothing).
    ///
    /// Returns whether the clicked node is selected afterwards — i.e. whether
    /// the press should go on to drag the selection. Clicking an already
    /// selected node keeps the whole selection, so several nodes can be
    /// dragged together; Shift toggles instead of replacing.
    pub fn click(&mut self, hit: Option<NodeId>, additive: bool) -> bool {
        match (hit, additive) {
            (Some(id), false) => {
                if !self.contains(id) {
                    self.set([id]);
                }
                true
            }
            (Some(id), true) => {
                self.toggle(id);
                self.contains(id)
            }
            (None, false) => {
                self.clear();
                false
            }
            (None, true) => false,
        }
    }

    /// Drop ids that are no longer in the tree — after undoing an insert, a
    /// delete, or loading a different document.
    pub fn retain_attached(&mut self, doc: &Document) {
        self.ids
            .retain(|&id| doc.contains(id) && doc.is_attached(id));
    }
}
