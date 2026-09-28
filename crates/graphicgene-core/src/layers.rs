//! Layer-panel operations that change the tree: moving layers, stepping
//! them through the stacking order, grouping and ungrouping.
//!
//! Each builds one `Command::Batch`, so each is one undo step; the session
//! applies it and decides what is selected afterwards. A node that moves to
//! another parent stays where it is on the page: its transform is rewritten
//! for the new parent, as moving a layer in or out of a group does in Figma.
//!
//! The panel lists the topmost layer first, while children are stored in
//! paint order, bottom first. `Drop` and `Arrange` speak the panel's terms.

use std::collections::HashMap;

use crate::command::Command;
use crate::doc::Document;
use crate::error::{CoreError, Result};
use crate::geom::Affine;
use crate::node::{NodeId, NodeKind};

/// Where dragged layers land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drop {
    /// The row above this one: in front of it, in its parent.
    Above(NodeId),
    /// The row below this one: behind it, in its parent.
    Below(NodeId),
    /// Into this group, in front of what it already holds.
    Inside(NodeId),
}

/// A step through the stacking order, within each node's own parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrange {
    Forward,
    Backward,
    ToFront,
    ToBack,
}

/// Move `ids` to `drop`, keeping their order. `None` when that changes
/// nothing or cannot be done: next to a row being moved, or into a moved
/// group's own subtree.
pub fn move_command(doc: &Document, ids: &[NodeId], drop: Drop) -> Result<Option<Command>> {
    let moving = roots(doc, ids)?;
    let (parent, anchor) = match drop {
        Drop::Above(id) | Drop::Below(id) => match doc.get(id)?.common.parent {
            Some(parent) => (parent, Some(id)),
            None => return Ok(None),
        },
        Drop::Inside(id) => (id, None),
    };
    if moving.is_empty()
        || doc.get(parent)?.children().is_none()
        || anchor.is_some_and(|a| moving.contains(&a))
    {
        return Ok(None);
    }
    for &id in &moving {
        if id == parent || is_ancestor(doc, id, parent)? {
            return Ok(None);
        }
    }

    let children = doc.children_of(parent)?;
    let remaining: Vec<NodeId> = children
        .iter()
        .copied()
        .filter(|c| !moving.contains(c))
        .collect();
    let index = match (drop, anchor) {
        (Drop::Above(_), Some(a)) => position(&remaining, a)? + 1,
        (Drop::Below(_), Some(a)) => position(&remaining, a)?,
        _ => remaining.len(),
    };
    let mut result = remaining;
    result.splice(index..index, moving.iter().copied());
    if result == children {
        return Ok(None);
    }

    let parent_world = doc.world_transform(parent)?;
    let mut commands: Vec<Command> = moving.iter().map(|&id| Command::Detach { id }).collect();
    for (offset, &id) in moving.iter().enumerate() {
        commands.push(Command::Attach {
            id,
            parent,
            index: index + offset,
        });
        if let Some(transform) = reparented(doc, id, parent_world)? {
            commands.push(Command::SetTransform { id, transform });
        }
    }
    Ok(Some(Command::Batch(commands)))
}

/// Step `ids` through the stacking order, each within its own parent: past
/// the next sibling that is not moving, or all the way. `None` when nothing
/// can move further.
pub fn arrange_command(
    doc: &Document,
    ids: &[NodeId],
    arrange: Arrange,
) -> Result<Option<Command>> {
    let moving = roots(doc, ids)?;
    let mut parents = Vec::new();
    for &id in &moving {
        let parent = parent_of(doc, id)?;
        if !parents.contains(&parent) {
            parents.push(parent);
        }
    }
    let picked = |id: &NodeId| moving.contains(id);
    let mut commands = Vec::new();
    for parent in parents {
        let old = doc.children_of(parent)?;
        let mut new = old.to_vec();
        match arrange {
            // From the top down, so a run of moving siblings moves as one.
            Arrange::Forward => {
                for i in (0..new.len().saturating_sub(1)).rev() {
                    if picked(&new[i]) && !picked(&new[i + 1]) {
                        new.swap(i, i + 1);
                    }
                }
            }
            Arrange::Backward => {
                for i in 1..new.len() {
                    if picked(&new[i]) && !picked(&new[i - 1]) {
                        new.swap(i - 1, i);
                    }
                }
            }
            // Stable sorts: the moving nodes keep their order among themselves.
            Arrange::ToFront => new.sort_by_key(|id| picked(id)),
            Arrange::ToBack => new.sort_by_key(|id| !picked(id)),
        }
        commands.extend(reorder(parent, old, &new));
    }
    Ok((!commands.is_empty()).then_some(Command::Batch(commands)))
}

/// Put `ids` into `group` — an empty group already in the arena, not yet
/// attached — at the place of the topmost of them, keeping everything
/// where it is on the page. `None` when there is nothing to group.
pub fn group_command(doc: &Document, ids: &[NodeId], group: NodeId) -> Result<Option<Command>> {
    let moving = roots(doc, ids)?;
    let Some(&top) = moving.last() else {
        return Ok(None);
    };
    let parent = parent_of(doc, top)?;
    let children = doc.children_of(parent)?;
    // The topmost node's place, counted without the nodes leaving.
    let index = children[..position(children, top)?]
        .iter()
        .filter(|c| !moving.contains(c))
        .count();
    let group_world = doc.world_transform(parent)? * doc.get(group)?.common.transform;

    let mut commands: Vec<Command> = moving.iter().map(|&id| Command::Detach { id }).collect();
    commands.push(Command::Attach {
        id: group,
        parent,
        index,
    });
    for (index, &id) in moving.iter().enumerate() {
        commands.push(Command::Attach {
            id,
            parent: group,
            index,
        });
        if let Some(transform) = reparented(doc, id, group_world)? {
            commands.push(Command::SetTransform { id, transform });
        }
    }
    Ok(Some(Command::Batch(commands)))
}

/// Dissolve the groups among `ids`: their children take each group's place
/// in its parent, with the group's transform and opacity folded into their
/// own, and hidden or locked if the group was. Returns the command and the
/// children that came out; `None` when no group is listed.
pub fn ungroup_command(doc: &Document, ids: &[NodeId]) -> Result<Option<(Command, Vec<NodeId>)>> {
    let mut groups = Vec::new();
    for id in roots(doc, ids)? {
        if let NodeKind::Group(_) = doc.get(id)?.kind {
            groups.push(id);
        }
    }
    if groups.is_empty() {
        return Ok(None);
    }
    // Topmost first: dissolving a group shifts the places above it, never
    // the ones below, where the groups still to go are.
    groups.reverse();

    let mut commands = Vec::new();
    let mut freed = Vec::new();
    for group in groups {
        let common = &doc.get(group)?.common;
        let parent = parent_of(doc, group)?;
        let index = position(doc.children_of(parent)?, group)?;
        let children = doc.children_of(group)?;
        commands.extend(children.iter().map(|&id| Command::Detach { id }));
        commands.push(Command::Detach { id: group });
        for (offset, &id) in children.iter().enumerate() {
            let child = &doc.get(id)?.common;
            commands.push(Command::Attach {
                id,
                parent,
                index: index + offset,
            });
            if common.transform != Affine::IDENTITY {
                let transform = common.transform * child.transform;
                commands.push(Command::SetTransform { id, transform });
            }
            if common.opacity != 1.0 {
                let opacity = common.opacity * child.opacity;
                commands.push(Command::SetOpacity { id, opacity });
            }
            if !common.visible && child.visible {
                commands.push(Command::SetVisible { id, visible: false });
            }
            if common.locked && !child.locked {
                commands.push(Command::SetLocked { id, locked: true });
            }
        }
        freed.extend_from_slice(children);
    }
    Ok(Some((Command::Batch(commands), freed)))
}

/// `ids` in paint order, bottom first, leaving out the root, nodes not in
/// the tree, repeats, and any node inside another listed one — it moves
/// with its ancestor.
pub fn roots(doc: &Document, ids: &[NodeId]) -> Result<Vec<NodeId>> {
    let order: HashMap<NodeId, usize> = doc
        .walk()
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id, i))
        .collect();
    let mut out = Vec::with_capacity(ids.len());
    for &id in ids {
        if id == doc.root() || !order.contains_key(&id) || out.contains(&id) {
            continue;
        }
        let mut inside = false;
        for &other in ids {
            if other != id && is_ancestor(doc, other, id)? {
                inside = true;
                break;
            }
        }
        if !inside {
            out.push(id);
        }
    }
    out.sort_by_key(|id| order[id]);
    Ok(out)
}

/// Whether `ancestor` is `id`'s parent, or its parent's parent, and so on.
pub fn is_ancestor(doc: &Document, ancestor: NodeId, id: NodeId) -> Result<bool> {
    let mut cursor = doc.get(id)?.common.parent;
    while let Some(current) = cursor {
        if current == ancestor {
            return Ok(true);
        }
        cursor = doc.get(current)?.common.parent;
    }
    Ok(false)
}

fn parent_of(doc: &Document, id: NodeId) -> Result<NodeId> {
    doc.get(id)?.common.parent.ok_or(CoreError::MissingNode(id))
}

fn position(children: &[NodeId], id: NodeId) -> Result<usize> {
    children
        .iter()
        .position(|&c| c == id)
        .ok_or(CoreError::MissingNode(id))
}

/// The transform that keeps `id` where it is on the page under a parent
/// whose world transform is `parent_world`; `None` when its own still does.
fn reparented(doc: &Document, id: NodeId, parent_world: Affine) -> Result<Option<Affine>> {
    let old = doc.parent_world_transform(id)?;
    if old == parent_world {
        return Ok(None);
    }
    let local = doc.get(id)?.common.transform;
    Ok(Some(parent_world.inverse() * old * local))
}

/// Commands that take `parent`'s children from `old` to `new`, the same
/// nodes in another order: detach the ones whose place changes, then attach
/// them bottom up, so each index they go to is already valid.
fn reorder(parent: NodeId, old: &[NodeId], new: &[NodeId]) -> Vec<Command> {
    let moved: Vec<(usize, NodeId)> = new
        .iter()
        .enumerate()
        .filter(|&(i, id)| old[i] != *id)
        .map(|(i, &id)| (i, id))
        .collect();
    let mut commands: Vec<Command> = moved
        .iter()
        .map(|&(_, id)| Command::Detach { id })
        .collect();
    commands.extend(
        moved
            .iter()
            .map(|&(index, id)| Command::Attach { id, parent, index }),
    );
    commands
}
