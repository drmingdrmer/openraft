use crate::error::NodeNotFound;
use crate::RaftTypeConfig;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub enum SendSnapshotError<C: RaftTypeConfig> {
    #[error("Can not send snapshot; error: {0}")]
    NodeNotFound(#[from] NodeNotFound<C>),
}
