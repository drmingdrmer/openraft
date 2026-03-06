use crate::RaftPrimitives;
use crate::log_id::raft_log_id::RaftLogId;
use crate::log_id::raft_log_id_ext::RaftLogIdExt;
use crate::log_id::ref_log_id::RefLogId;
use crate::type_config::alias::CommittedLeaderIdOf;

/// This helper trait extracts information from an `Option<T>` where T impls [`RaftLogId`].
pub(crate) trait OptionRaftLogIdExt<P>
where P: RaftPrimitives
{
    /// Returns the log index if it is not a `None`.
    fn index(&self) -> Option<u64>;

    /// Returns the next log index.
    ///
    /// If self is `None`, it returns 0.
    fn next_index(&self) -> u64;

    /// Returns the leader id that proposed this log id.
    ///
    /// In standard raft, committed leader id is just `term`.
    #[allow(dead_code)]
    fn committed_leader_id(&self) -> Option<&CommittedLeaderIdOf<P>>;

    /// Converts this `Option<T: RaftLogId>` into a reference-based log ID.
    ///
    /// Returns `Some(RefLogId)` if `self` is `Some(T)`, `None` otherwise.
    fn to_ref(&self) -> Option<RefLogId<'_, P>>;
}

impl<P, T> OptionRaftLogIdExt<P> for Option<T>
where
    P: RaftPrimitives,
    T: RaftLogId<P>,
{
    fn index(&self) -> Option<u64> {
        self.as_ref().map(|x| x.index())
    }

    fn next_index(&self) -> u64 {
        match self {
            None => 0,
            Some(log_id) => log_id.index() + 1,
        }
    }

    fn committed_leader_id(&self) -> Option<&CommittedLeaderIdOf<P>> {
        self.as_ref().map(|x| x.committed_leader_id())
    }

    fn to_ref(&self) -> Option<RefLogId<'_, P>> {
        self.as_ref().map(|x| x.to_ref())
    }
}
