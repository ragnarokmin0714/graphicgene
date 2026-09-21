//! graphicgene core: document model, commands, geometry and the layout pass.
//!
//! Two rules hold everywhere in this crate:
//!
//! 1. **No IO.** Core produces and consumes bytes; storage belongs to the app
//!    layer. This is what lets the same core run on the web (async IndexedDB)
//!    and on the desktop (sync `std::fs`).
//! 2. **No browser or OS APIs.** Platform differences go behind traits that the
//!    app layer implements.

pub mod color;
pub mod command;
pub mod doc;
pub mod error;
pub mod geom;
pub mod layout;
pub mod node;
pub mod project;

pub use color::LinearRgba;
pub use command::{Command, Journal};
pub use doc::Document;
pub use error::{CoreError, Result};
pub use node::{BlendMode, Node, NodeId, NodeKind};
