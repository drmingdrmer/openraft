use crate::log_id::raft_log_id::RaftLogId;
use crate::log_id::raft_log_id_ext::RaftLogIdExt;
use crate::log_id::ref_log_id::RefLogId;

/// This helper trait extracts information from an `Option<T>` where T impls [`RaftLogId`].
pub(crate) trait OptionRaftLogIdExt<T>
where T: RaftLogId
{
    /// Returns the leader id that proposed this log id.
    ///
    /// In standard raft, committed leader id is just `term`.
    #[allow(dead_code)]
    fn committed_leader_id(&self) -> Option<&T::CommittedLeaderId>;

    /// Converts this `Option<T: RaftLogId>` into a reference-based log ID.
    ///
    /// Returns `Some(RefLogId)` if `self` is `Some(T)`, `None` otherwise.
    fn to_ref(&self) -> Option<RefLogId<'_, T::CommittedLeaderId>>;
}

impl<T> OptionRaftLogIdExt<T> for Option<T>
where T: RaftLogId
{
    fn committed_leader_id(&self) -> Option<&T::CommittedLeaderId> {
        self.as_ref().map(|x| x.committed_leader_id())
    }

    fn to_ref(&self) -> Option<RefLogId<'_, T::CommittedLeaderId>> {
        self.as_ref().map(|x| x.to_ref())
    }
}
