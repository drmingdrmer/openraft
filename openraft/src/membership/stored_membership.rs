use std::fmt;

use crate::Membership;
use crate::RaftPrimitives;
use crate::display_ext::DisplayOption;
use crate::type_config::alias::LogIdOf;

/// This struct represents information about a membership config that has already been stored in the
/// raft logs.
///
/// It includes log id and a membership config. Such a record is used in the state machine or
/// snapshot to track the last membership and its log id. And it is also used as a return value for
/// functions that return membership and its log position.
///
/// It derives `Default` for building an uninitialized membership state, e.g., when a raft-node is
/// just created.
#[derive(Clone, Debug, Default)]
#[derive(PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub struct StoredMembership<P>
where P: RaftPrimitives
{
    /// The id of the log that stores this membership config
    log_id: Option<LogIdOf<P>>,

    /// Membership config
    membership: Membership<P>,
}

impl<P> StoredMembership<P>
where P: RaftPrimitives
{
    /// Create a new StoredMembership with the given log ID and membership configuration.
    pub fn new(log_id: Option<LogIdOf<P>>, membership: Membership<P>) -> Self {
        Self { log_id, membership }
    }

    /// Get the log ID at which this membership was stored.
    pub fn log_id(&self) -> &Option<LogIdOf<P>> {
        &self.log_id
    }

    /// Get the membership configuration.
    pub fn membership(&self) -> &Membership<P> {
        &self.membership
    }

    /// Get an iterator over the voter node IDs.
    pub fn voter_ids(&self) -> impl Iterator<Item = P::NodeId> {
        self.membership.voter_ids()
    }

    /// Get an iterator over all nodes (ID and node information).
    pub fn nodes(&self) -> impl Iterator<Item = (&P::NodeId, &P::Node)> {
        self.membership.nodes()
    }
}

impl<P> fmt::Display for StoredMembership<P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{log_id:{}, {}}}", DisplayOption(&self.log_id), self.membership)
    }
}
