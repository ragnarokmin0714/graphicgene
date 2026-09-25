//! The document: an arena of nodes plus a root.
//!
//! Nodes live in a `SlotMap` rather than an `Rc<RefCell<..>>` tree. Arena
//! storage is cache-friendly, serializable, and gives the stable identity that
//! components and any future collaboration model both depend on.
//!
//! The document also keeps a log of what changed since it was last asked
//! (`take_changes`). Every mutation goes through `get_mut`, `attach` or
//! `detach`, so recording there cannot be bypassed — which is what lets the
//! renderer redraw only what moved instead of trusting callers to report it.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

use crate::error::{CoreError, Result};
use crate::geom::{Affine, BezPath, Bounds, empty_bounds, union};
use crate::node::{Node, NodeId, NodeKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    nodes: SlotMap<NodeId, Node>,
    root: NodeId,
    /// Not saved: a document fresh from a file has changed "everything".
    #[serde(skip, default = "Changes::everything")]
    changes: Changes,
    /// Bumped on every attach and detach. Not saved.
    #[serde(skip)]
    structure_version: u64,
}

/// What changed in a document since the last `take_changes`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Nodes whose own fields may have changed — transform, path, paint,
    /// visibility and so on. Recorded conservatively: asking for a node
    /// mutably counts, whether or not anything was then written.
    pub nodes: BTreeSet<NodeId>,
    /// The tree's shape changed: a node was attached or detached.
    pub structure: bool,
    /// Treat everything as changed: a new document, or one just loaded.
    pub everything: bool,
}

impl Changes {
    pub fn everything() -> Self {
        Self {
            everything: true,
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.everything && !self.structure && self.nodes.is_empty()
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert(Node::group("Root"));
        Self {
            nodes,
            root,
            changes: Changes::everything(),
            structure_version: 0,
        }
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn get(&self, id: NodeId) -> Result<&Node> {
        self.nodes.get(id).ok_or(CoreError::MissingNode(id))
    }

    /// Mutable access to a node. Records the node as changed.
    pub fn get_mut(&mut self, id: NodeId) -> Result<&mut Node> {
        let node = self.nodes.get_mut(id).ok_or(CoreError::MissingNode(id))?;
        self.changes.nodes.insert(id);
        Ok(node)
    }

    /// Everything that changed since the last call, leaving the log empty.
    pub fn take_changes(&mut self) -> Changes {
        std::mem::take(&mut self.changes)
    }

    /// Counts attaches and detaches since the document was created or
    /// loaded: anything showing the tree's shape can cache on it.
    pub fn structure_version(&self) -> u64 {
        self.structure_version
    }

    fn structure_changed(&mut self, id: NodeId) {
        self.changes.structure = true;
        self.changes.nodes.insert(id);
        self.structure_version += 1;
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Put a node in the arena without attaching it to a parent.
    ///
    /// Undo works by *detaching* rather than deleting, so a node's `NodeId`
    /// survives undo/redo cycles and references to it stay valid.
    pub fn insert_detached(&mut self, node: Node) -> NodeId {
        self.nodes.insert(node)
    }

    pub fn attach(&mut self, id: NodeId, parent: NodeId, index: usize) -> Result<()> {
        if !self.nodes.contains_key(id) {
            return Err(CoreError::MissingNode(id));
        }
        let parent_node = self
            .nodes
            .get_mut(parent)
            .ok_or(CoreError::MissingNode(parent))?;
        let children = parent_node
            .children_mut()
            .ok_or(CoreError::NotAContainer { parent, child: id })?;
        if index > children.len() {
            return Err(CoreError::BadChildIndex {
                index,
                len: children.len(),
            });
        }
        children.insert(index, id);
        self.nodes[id].common.parent = Some(parent);
        self.structure_changed(id);
        Ok(())
    }

    /// Detach a node from its parent, leaving it in the arena.
    ///
    /// Returns where it was, so the operation can be inverted exactly.
    pub fn detach(&mut self, id: NodeId) -> Result<(NodeId, usize)> {
        let parent = self
            .nodes
            .get(id)
            .ok_or(CoreError::MissingNode(id))?
            .common
            .parent
            .ok_or(CoreError::MissingNode(id))?;
        let children = self.nodes[parent]
            .children_mut()
            .ok_or(CoreError::NotAContainer { parent, child: id })?;
        let index = children
            .iter()
            .position(|c| *c == id)
            .ok_or(CoreError::MissingNode(id))?;
        children.remove(index);
        self.nodes[id].common.parent = None;
        self.structure_changed(id);
        Ok((parent, index))
    }

    /// The path of a vector node.
    pub fn vector_path(&self, id: NodeId) -> Result<&BezPath> {
        match &self.get(id)?.kind {
            NodeKind::Vector(vector) => Ok(&vector.path),
            _ => Err(CoreError::NotAVector(id)),
        }
    }

    /// Replace a vector node's path without going through the journal.
    ///
    /// Only for previews during a gesture, which then commit the final path
    /// as a `Command::SetPath` (or restore the original on cancel). Anything
    /// else must use the command, or undo loses track.
    pub(crate) fn write_path(&mut self, id: NodeId, path: BezPath) -> Result<()> {
        match &mut self.get_mut(id)?.kind {
            NodeKind::Vector(vector) => {
                vector.path = path;
                Ok(())
            }
            _ => Err(CoreError::NotAVector(id)),
        }
    }

    /// Whether `id` is reachable from the root. Undo detaches rather than
    /// deletes, so a live id is not necessarily part of the document.
    pub fn is_attached(&self, id: NodeId) -> bool {
        let mut cursor = id;
        loop {
            if cursor == self.root {
                return true;
            }
            match self.nodes.get(cursor).and_then(|node| node.common.parent) {
                Some(parent) => cursor = parent,
                None => return false,
            }
        }
    }

    /// World transform of `id`'s parent: what maps its local transform into
    /// document space. Identity for the root.
    pub fn parent_world_transform(&self, id: NodeId) -> Result<Affine> {
        match self.get(id)?.common.parent {
            Some(parent) => self.world_transform(parent),
            None => Ok(Affine::IDENTITY),
        }
    }

    pub fn children_of(&self, id: NodeId) -> Result<&[NodeId]> {
        Ok(self.get(id)?.children().unwrap_or(&[]))
    }

    /// Accumulated transform from the document root down to `id`.
    pub fn world_transform(&self, id: NodeId) -> Result<Affine> {
        let mut transform = Affine::IDENTITY;
        let mut chain = Vec::new();
        let mut cursor = Some(id);
        while let Some(current) = cursor {
            let node = self.get(current)?;
            chain.push(node.common.transform);
            cursor = node.common.parent;
        }
        for t in chain.iter().rev() {
            transform *= *t;
        }
        Ok(transform)
    }

    /// Bounds of a subtree in document space.
    pub fn world_bounds(&self, id: NodeId) -> Result<Bounds> {
        let transform = self.world_transform(id)?;
        let node = self.get(id)?;
        let mut bounds = match node.local_bounds() {
            Some(b) => transform.transform_rect_bbox(b),
            None => empty_bounds(),
        };
        for child in self.children_of(id)? {
            bounds = union(bounds, self.world_bounds(*child)?);
        }
        Ok(bounds)
    }

    /// Depth-first walk from the root, parents before children.
    ///
    /// This is the order the render scene is built in.
    pub fn walk(&self) -> Vec<NodeId> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            out.push(id);
            if let Some(children) = self.nodes[id].children() {
                for child in children.iter().rev() {
                    stack.push(*child);
                }
            }
        }
        out
    }

    /// Drop arena entries that are not reachable from the root.
    ///
    /// Detached-but-live nodes are what undo depends on, so this must only run
    /// when the undo history is being discarded (save, close, explicit purge).
    pub fn purge_unreachable(&mut self) {
        let reachable: std::collections::HashSet<NodeId> = self.walk().into_iter().collect();
        self.nodes.retain(|id, _| reachable.contains(&id));
    }
}
