use std::fmt::Display;
use std::fmt::Formatter;

use crate::LogId;
use crate::RaftPrimitives;
use crate::log_id::raft_log_id::RaftLogId;
use crate::type_config::alias::CommittedLeaderIdOf;

/// A reference to a log id, combining a reference to a committed leader ID and an index.
/// Committed leader ID is the `term` in standard Raft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RefLogId<'k, P>
where P: RaftPrimitives
{
    pub(crate) leader_id: &'k CommittedLeaderIdOf<P>,
    pub(crate) index: u64,
}

impl<'l, P> RefLogId<'l, P>
where P: RaftPrimitives
{
    /// Create a new reference log id.
    pub(crate) fn new(leader_id: &'l CommittedLeaderIdOf<P>, index: u64) -> Self {
        RefLogId { leader_id, index }
    }

    /// Return the committed leader ID of this log id, which is `term` in standard Raft.
    pub(crate) fn committed_leader_id(&self) -> &'l CommittedLeaderIdOf<P> {
        self.leader_id
    }

    /// Return the index of this log id.
    pub(crate) fn index(&self) -> u64 {
        self.index
    }

    /// Convert this reference type to an owned log id.
    pub(crate) fn into_log_id(self) -> LogId<P> {
        LogId::<P>::new(self.leader_id.clone(), self.index)
    }
}

impl<P> Display for RefLogId<'_, P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.committed_leader_id(), self.index())
    }
}

impl<P> RaftLogId<P> for RefLogId<'_, P>
where P: RaftPrimitives
{
    fn new(_leader_id: CommittedLeaderIdOf<P>, _index: u64) -> Self {
        unreachable!("RefLogId does not own the leader id, so it cannot be created from it.")
    }

    fn committed_leader_id(&self) -> &CommittedLeaderIdOf<P> {
        self.leader_id
    }

    fn index(&self) -> u64 {
        self.index
    }
}
