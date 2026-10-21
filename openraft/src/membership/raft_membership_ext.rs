use std::collections::BTreeSet;

use crate::membership::NodeRole;
use crate::membership::RaftMembership;
use crate::quorum::QuorumSet;

/// Queries derived from the required methods of [`RaftMembership`].
pub(crate) trait RaftMembershipExt: RaftMembership {
    /// Returns whether `node_id` is a known node of this configuration.
    fn contains(&self, node_id: &Self::NodeId) -> bool {
        let node = self.get_node(node_id);
        node.is_some()
    }

    /// Returns whether `node_id` may start an election under this configuration.
    fn is_campaigner(&self, node_id: &Self::NodeId) -> bool {
        let Some(node) = self.get_node(node_id) else {
            return false;
        };
        node.role.contains(NodeRole::Campaigner)
    }

    /// Returns whether `node_id` is in the read quorum set.
    fn is_read_voter(&self, node_id: &Self::NodeId) -> bool {
        let Some(node) = self.get_node(node_id) else {
            return false;
        };
        node.is_read_voter
    }

    /// Returns whether `node_id` is in the write quorum set.
    fn is_write_voter(&self, node_id: &Self::NodeId) -> bool {
        let Some(node) = self.get_node(node_id) else {
            return false;
        };
        node.is_write_voter
    }

    /// Returns whether `node_id` is a known node that receives logs and has no campaign or voter
    /// role.
    fn is_learner(&self, node_id: &Self::NodeId) -> bool {
        let Some(node) = self.get_node(node_id) else {
            return false;
        };
        node.is_learner()
    }

    /// Returns the IDs in the read or the write quorum set, in ascending order.
    fn voter_ids(&self) -> impl Iterator<Item = Self::NodeId> {
        let read_ids = self.read_quorum_set().ids();
        let write_ids = self.write_quorum_set().ids();
        let ids: BTreeSet<_> = read_ids.chain(write_ids).collect();
        ids.into_iter()
    }

    /// Returns the known node IDs outside the write quorum set.
    fn non_write_voter_ids(&self) -> impl Iterator<Item = Self::NodeId> {
        self.node_ids().filter(|id| !self.is_write_voter(id))
    }

    /// Returns the first node ID that both configurations know but describe with a different
    /// node.
    ///
    /// A node ID that only one of the two configurations knows is skipped: adding a node and
    /// removing a node are not metadata changes.
    fn find_changed_node_metadata(&self, other: &Self) -> Option<Self::NodeId> {
        for node_id in self.node_ids() {
            let Some(node) = self.get_node(&node_id) else {
                continue;
            };
            let Some(other_node) = other.get_node(&node_id) else {
                continue;
            };

            if other_node.node != node.node {
                return Some(node_id);
            }
        }

        None
    }
}

impl<T> RaftMembershipExt for T where T: RaftMembership {}
