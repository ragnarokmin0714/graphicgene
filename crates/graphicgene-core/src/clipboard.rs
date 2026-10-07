//! Copy, paste and duplicate: nodes as self-contained JSON, pasted with
//! fresh ids.
//!
//! Core does no IO. Copying returns text and pasting takes it; the app layer
//! moves it through the system clipboard. The text is marked and versioned,
//! so a paste can tell its own from whatever else is on the clipboard.
//!
//! Copied nodes carry their place on the page — the top-level ones their
//! world transform — so a paste lands exactly where the copy came from,
//! into this document or another one, as in Figma.

use serde::{Deserialize, Serialize};

use crate::command::Command;
use crate::doc::Document;
use crate::error::Result;
use crate::layers;
use crate::node::{Node, NodeId, NodeKind};

/// Marks clipboard text as graphicgene nodes.
const MARK: &str = "graphicgene/nodes";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Clip {
    #[serde(rename = "type")]
    mark: String,
    version: u32,
    nodes: Vec<Tree>,
}

/// A node with its descendants inline, instead of as ids into an arena.
#[derive(Serialize, Deserialize)]
struct Tree {
    node: Node,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    children: Vec<Tree>,
}

/// The clipboard text for `ids` and everything inside them; `None` when
/// there is nothing to copy.
pub fn copy(doc: &Document, ids: &[NodeId]) -> Result<Option<String>> {
    let mut nodes = Vec::new();
    for id in layers::roots(doc, ids)? {
        let mut tree = tree(doc, id)?;
        tree.node.common.transform = doc.world_transform(id)?;
        nodes.push(tree);
    }
    if nodes.is_empty() {
        return Ok(None);
    }
    let clip = Clip {
        mark: MARK.to_owned(),
        version: VERSION,
        nodes,
    };
    Ok(Some(serde_json::to_string(&clip)?))
}

/// Paste clipboard text into `parent` at `index` among its children — on
/// top of it with `None`: the nodes enter the arena with fresh ids, and the
/// command attaches them, each where it was copied from on the page.
/// Returns the command and the new top-level nodes, or `None` for text that
/// is not graphicgene nodes — anything at all may be on the clipboard, and
/// it is not an error.
pub fn paste(
    doc: &mut Document,
    text: &str,
    parent: NodeId,
    index: Option<usize>,
) -> Result<Option<(Command, Vec<NodeId>)>> {
    let Ok(clip) = serde_json::from_str::<Clip>(text) else {
        return Ok(None);
    };
    if clip.mark != MARK || clip.version > VERSION || clip.nodes.is_empty() {
        return Ok(None);
    }
    let index = index.unwrap_or(doc.children_of(parent)?.len());
    // Copied nodes carry world transforms; inside a moved group they need
    // local ones that land them in the same place.
    let to_local = doc.world_transform(parent)?.inverse();
    let mut commands = Vec::new();
    let mut pasted = Vec::new();
    for (offset, mut tree) in clip.nodes.into_iter().enumerate() {
        tree.node.common.transform = to_local * tree.node.common.transform;
        let id = instantiate(doc, tree, &mut commands);
        commands.push(Command::Attach {
            id,
            parent,
            index: index + offset,
        });
        pasted.push(id);
    }
    Ok(Some((Command::Batch(commands), pasted)))
}

/// Copy `ids` in place, each copy directly above its original in the same
/// parent. Returns the command and the copies; `None` with nothing to copy.
pub fn duplicate(doc: &mut Document, ids: &[NodeId]) -> Result<Option<(Command, Vec<NodeId>)>> {
    let mut originals = layers::roots(doc, ids)?;
    if originals.is_empty() {
        return Ok(None);
    }
    // Topmost first: a copy shifts the places above its original, never the
    // ones below, where the originals still to copy are.
    originals.reverse();
    let mut commands = Vec::new();
    let mut copies = Vec::new();
    for original in originals {
        let parent = doc.get(original)?.common.parent;
        let Some(parent) = parent else { continue };
        let index = doc
            .children_of(parent)?
            .iter()
            .position(|&c| c == original)
            .map_or(0, |i| i + 1);
        let tree = tree(doc, original)?;
        let id = instantiate(doc, tree, &mut commands);
        commands.push(Command::Attach { id, parent, index });
        copies.push(id);
    }
    copies.reverse();
    Ok(Some((Command::Batch(commands), copies)))
}

/// `id` and its descendants, cut loose from the arena: no parent, and a
/// group's children inline rather than as ids.
fn tree(doc: &Document, id: NodeId) -> Result<Tree> {
    let mut node = doc.get(id)?.clone();
    node.common.parent = None;
    let mut children = Vec::new();
    if let NodeKind::Group(group) = &mut node.kind {
        for child in std::mem::take(&mut group.children) {
            children.push(tree(doc, child)?);
        }
    }
    Ok(Tree { node, children })
}

/// Put a tree's nodes in the arena, unattached, and add the commands that
/// attach each child to its parent. Returns the top node's id.
///
/// The tree came off the clipboard, so it is not trusted to be well formed:
/// ids inside it are dropped, and so are children under anything but a
/// group.
fn instantiate(doc: &mut Document, tree: Tree, commands: &mut Vec<Command>) -> NodeId {
    let Tree { mut node, children } = tree;
    node.common.parent = None;
    let children = match &mut node.kind {
        NodeKind::Group(group) => {
            group.children.clear();
            children
        }
        NodeKind::Vector(_) | NodeKind::Text(_) => Vec::new(),
    };
    let id = doc.insert_detached(node);
    for (index, child) in children.into_iter().enumerate() {
        let child = instantiate(doc, child, commands);
        commands.push(Command::Attach {
            id: child,
            parent: id,
            index,
        });
    }
    id
}
