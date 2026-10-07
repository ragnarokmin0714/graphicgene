//! Document mutations.
//!
//! Every change to a document is a `Command`. Applying one returns its exact
//! inverse, which is what the journal stores for undo.
//!
//! `Command` is a serializable enum, not a boxed trait object, for the same
//! reason `NodeKind` is: `dyn` and serde fight each other, and a wire-format
//! for commands is the thing that keeps a networked design possible later.
//!
//! Note what this does *not* decide: multiplayer needs a conflict model (a tree
//! CRDT for node moves, or a server-authoritative sequencer). The journal buys
//! undo/redo and replay, nothing more. See CLAUDE.md.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::error::{CoreError, Result};
use crate::geom::{Affine, BezPath};
use crate::node::{Node, NodeId, NodeKind, Stroke};
use crate::paint::Paint;
use crate::text::TextStyle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    /// Add a brand-new node. Inverts to `Detach`, which keeps the id alive.
    InsertNode {
        parent: NodeId,
        index: usize,
        node: Node,
    },
    Attach {
        id: NodeId,
        parent: NodeId,
        index: usize,
    },
    Detach {
        id: NodeId,
    },
    SetTransform {
        id: NodeId,
        transform: Affine,
    },
    SetOpacity {
        id: NodeId,
        opacity: f32,
    },
    SetVisible {
        id: NodeId,
        visible: bool,
    },
    SetLocked {
        id: NodeId,
        locked: bool,
    },
    Rename {
        id: NodeId,
        name: String,
    },
    SetFill {
        id: NodeId,
        fill: Option<Paint>,
    },
    SetStroke {
        id: NodeId,
        stroke: Option<Stroke>,
    },
    SetPath {
        id: NodeId,
        path: BezPath,
    },
    SetText {
        id: NodeId,
        content: String,
    },
    SetTextStyle {
        id: NodeId,
        style: TextStyle,
    },
    /// Several commands as one undo step — a drag that moves five nodes, or a
    /// delete of the whole selection. All or nothing: if one fails, the ones
    /// already applied are rolled back.
    Batch(Vec<Command>),
}

impl Command {
    /// Apply to `doc` and return the command that undoes it.
    pub fn apply(&self, doc: &mut Document) -> Result<Command> {
        match self {
            Command::InsertNode {
                parent,
                index,
                node,
            } => {
                let id = doc.insert_detached(node.clone());
                doc.attach(id, *parent, *index)?;
                Ok(Command::Detach { id })
            }

            Command::Attach { id, parent, index } => {
                doc.attach(*id, *parent, *index)?;
                Ok(Command::Detach { id: *id })
            }

            Command::Detach { id } => {
                let (parent, index) = doc.detach(*id)?;
                Ok(Command::Attach {
                    id: *id,
                    parent,
                    index,
                })
            }

            Command::SetTransform { id, transform } => {
                let node = doc.get_mut(*id)?;
                let previous = node.common.transform;
                node.common.transform = *transform;
                Ok(Command::SetTransform {
                    id: *id,
                    transform: previous,
                })
            }

            Command::SetOpacity { id, opacity } => {
                let node = doc.get_mut(*id)?;
                let previous = node.common.opacity;
                node.common.opacity = opacity.clamp(0.0, 1.0);
                Ok(Command::SetOpacity {
                    id: *id,
                    opacity: previous,
                })
            }

            Command::SetVisible { id, visible } => {
                let node = doc.get_mut(*id)?;
                let previous = node.common.visible;
                node.common.visible = *visible;
                Ok(Command::SetVisible {
                    id: *id,
                    visible: previous,
                })
            }

            Command::SetLocked { id, locked } => {
                let node = doc.get_mut(*id)?;
                let previous = node.common.locked;
                node.common.locked = *locked;
                Ok(Command::SetLocked {
                    id: *id,
                    locked: previous,
                })
            }

            Command::Rename { id, name } => {
                let node = doc.get_mut(*id)?;
                let previous = std::mem::replace(&mut node.common.name, name.clone());
                Ok(Command::Rename {
                    id: *id,
                    name: previous,
                })
            }

            Command::SetFill { id, fill } => {
                let node = doc.get_mut(*id)?;
                let slot = match &mut node.kind {
                    NodeKind::Vector(v) => &mut v.fill,
                    NodeKind::Text(t) => &mut t.fill,
                    NodeKind::Group(_) => return Err(CoreError::NotAVector(*id)),
                };
                let previous = std::mem::replace(slot, fill.clone());
                Ok(Command::SetFill {
                    id: *id,
                    fill: previous,
                })
            }

            Command::SetStroke { id, stroke } => {
                let node = doc.get_mut(*id)?;
                match &mut node.kind {
                    NodeKind::Vector(v) => {
                        let previous = std::mem::replace(&mut v.stroke, *stroke);
                        Ok(Command::SetStroke {
                            id: *id,
                            stroke: previous,
                        })
                    }
                    NodeKind::Group(_) | NodeKind::Text(_) => Err(CoreError::NotAVector(*id)),
                }
            }

            Command::SetPath { id, path } => {
                let node = doc.get_mut(*id)?;
                match &mut node.kind {
                    NodeKind::Vector(v) => {
                        let previous = std::mem::replace(&mut v.path, path.clone());
                        Ok(Command::SetPath {
                            id: *id,
                            path: previous,
                        })
                    }
                    NodeKind::Group(_) | NodeKind::Text(_) => Err(CoreError::NotAVector(*id)),
                }
            }

            Command::SetText { id, content } => {
                let NodeKind::Text(text) = &mut doc.get_mut(*id)?.kind else {
                    return Err(CoreError::NotText(*id));
                };
                let previous = std::mem::replace(&mut text.content, content.clone());
                Ok(Command::SetText {
                    id: *id,
                    content: previous,
                })
            }

            Command::SetTextStyle { id, style } => {
                let NodeKind::Text(text) = &mut doc.get_mut(*id)?.kind else {
                    return Err(CoreError::NotText(*id));
                };
                let previous = std::mem::replace(&mut text.style, style.clone());
                Ok(Command::SetTextStyle {
                    id: *id,
                    style: previous,
                })
            }

            Command::Batch(commands) => {
                let mut inverses = Vec::with_capacity(commands.len());
                for command in commands {
                    match command.apply(doc) {
                        Ok(inverse) => inverses.push(inverse),
                        Err(e) => {
                            // A half-applied batch is exactly the state undo
                            // cannot describe, so put the document back. The
                            // inverses were produced by successful applies, so
                            // they apply cleanly in reverse.
                            for inverse in inverses.iter().rev() {
                                let _ = inverse.apply(doc);
                            }
                            return Err(e);
                        }
                    }
                }
                inverses.reverse();
                Ok(Command::Batch(inverses))
            }
        }
    }

    /// The node this command touches, for render invalidation.
    pub fn target(&self) -> Option<NodeId> {
        match self {
            Command::InsertNode { parent, .. } => Some(*parent),
            Command::Attach { id, .. }
            | Command::Detach { id }
            | Command::SetTransform { id, .. }
            | Command::SetOpacity { id, .. }
            | Command::SetVisible { id, .. }
            | Command::SetLocked { id, .. }
            | Command::Rename { id, .. }
            | Command::SetFill { id, .. }
            | Command::SetStroke { id, .. }
            | Command::SetPath { id, .. }
            | Command::SetText { id, .. }
            | Command::SetTextStyle { id, .. } => Some(*id),
            Command::Batch(commands) => commands.first().and_then(Command::target),
        }
    }
}

/// Undo/redo history.
#[derive(Debug, Default)]
pub struct Journal {
    undo: Vec<Command>,
    redo: Vec<Command>,
    /// Commands applied so far, counting undos and redos. Never reset, so it
    /// only ever grows: views can cache on it.
    ops: u64,
}

impl Journal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn execute(&mut self, doc: &mut Document, command: Command) -> Result<()> {
        let inverse = command.apply(doc)?;
        self.undo.push(inverse);
        self.redo.clear();
        self.ops += 1;
        Ok(())
    }

    pub fn undo(&mut self, doc: &mut Document) -> Result<bool> {
        let Some(command) = self.undo.pop() else {
            return Ok(false);
        };
        let inverse = command.apply(doc)?;
        self.redo.push(inverse);
        self.ops += 1;
        Ok(true)
    }

    pub fn redo(&mut self, doc: &mut Document) -> Result<bool> {
        let Some(command) = self.redo.pop() else {
            return Ok(false);
        };
        let inverse = command.apply(doc)?;
        self.undo.push(inverse);
        self.ops += 1;
        Ok(true)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Commands applied so far, including undos and redos.
    pub fn ops(&self) -> u64 {
        self.ops
    }

    /// Forget the history. `ops` keeps counting.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
