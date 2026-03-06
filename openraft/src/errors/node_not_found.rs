use crate::RaftPrimitives;
use crate::errors::Operation;

/// Error indicating a node was not found in the cluster.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
#[error("Node {node_id} not found when: ({operation})")]
pub struct NodeNotFound<P: RaftPrimitives> {
    /// The node ID that was not found.
    pub node_id: P::NodeId,
    /// The operation that was being attempted when the node was not found.
    pub operation: Operation,
}

impl<P: RaftPrimitives> NodeNotFound<P> {
    /// Create a new NodeNotFound error.
    pub fn new(node_id: P::NodeId, operation: Operation) -> Self {
        Self { node_id, operation }
    }
}
