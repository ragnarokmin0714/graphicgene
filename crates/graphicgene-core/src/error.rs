use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("node {0:?} does not exist")]
    MissingNode(crate::node::NodeId),

    #[error("node {child:?} cannot be inserted into {parent:?}: parent holds no children")]
    NotAContainer {
        parent: crate::node::NodeId,
        child: crate::node::NodeId,
    },

    #[error("child index {index} is out of range (parent holds {len})")]
    BadChildIndex { index: usize, len: usize },

    #[error("project file version {found} is newer than this build supports ({supported})")]
    UnsupportedVersion { found: u32, supported: u32 },

    #[error("project file could not be parsed: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
