//! The project file: versioned JSON, backward compatible from v0.1 onward.
//!
//! Core performs no IO — these functions move between a `Document` and bytes,
//! and the app layer decides where those bytes live. IndexedDB is async and
//! `std::fs` is sync; a core that assumed either could not run on the other
//! platform.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::doc::Document;
use crate::error::{CoreError, Result};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub document: Document,

    /// Fields written by a newer build than this one.
    ///
    /// Kept so that opening and re-saving a project in an older build does not
    /// silently destroy data it did not understand.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub unknown: Map<String, Value>,
}

impl Project {
    pub fn new(document: Document) -> Self {
        Self {
            version: FORMAT_VERSION,
            document,
            unknown: Map::new(),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(text: &str) -> Result<Self> {
        // The version is read before anything else, so that a file from a
        // newer build produces a clear error rather than a confusing one.
        let probe: Map<String, Value> = serde_json::from_str(text)?;
        let found = probe.get("version").and_then(Value::as_u64).unwrap_or(0) as u32;
        if found > FORMAT_VERSION {
            return Err(CoreError::UnsupportedVersion {
                found,
                supported: FORMAT_VERSION,
            });
        }
        Ok(serde_json::from_str(text)?)
    }
}
