use std::fmt;

use openraft_macros::since;

use crate::log_id::raft_log_id_from_parts::RaftLogIdFromParts;
use crate::vote::RaftCommittedLeaderId;

/// Log id is the globally unique identifier of a log entry.
///
/// Equal log id means the same log entry. Both owned and borrowed log IDs implement this trait.
/// To construct a log ID from its owned parts, use [`RaftLogIdFromParts`].
#[since(version = "0.10.2", change = "moved `new` to `RaftLogIdFromParts`")]
#[since(
    version = "0.10.0",
    change = "removed `C: RaftTypeConfig` generic parameter, added `CommittedLeaderId` associated type"
)]
pub trait RaftLogId
where Self: Eq + Clone + fmt::Debug
{
    /// The committed leader ID type used by this log ID.
    #[since(version = "0.10.0")]
    type CommittedLeaderId: RaftCommittedLeaderId;

    /// Returns a reference to the leader id that proposed this log id.
    ///
    /// When a `LeaderId` is committed, some of its data can be discarded.
    /// For example, a leader id in standard raft is `(term, node_id)`, but a log id does not have
    /// to store the `node_id`, because in standard raft there is at most one leader that can be
    /// established.
    fn committed_leader_id(&self) -> &Self::CommittedLeaderId;

    /// Returns the index of the log id.
    fn index(&self) -> u64;

    /// Converts this log ID into a type that can be constructed from the same parts.
    ///
    /// A reference can be the source, but it cannot be the destination:
    ///
    /// ```compile_fail,E0277
    /// use openraft::log_id::RaftLogId;
    ///
    /// let log_id = (1_u64, 5_u64);
    /// let _: &(u64, u64) = log_id.to_type();
    /// ```
    #[since(version = "0.10.2", change = "requires `RaftLogIdFromParts` on the destination")]
    fn to_type<T>(&self) -> T
    where T: RaftLogIdFromParts<CommittedLeaderId = Self::CommittedLeaderId> {
        T::new(self.committed_leader_id().clone(), self.index())
    }

    /// Returns the parts of this log ID as a tuple of (committed_leader_id, index).
    fn log_id_parts(&self) -> (&Self::CommittedLeaderId, u64) {
        (self.committed_leader_id(), self.index())
    }
}

impl<T> RaftLogId for &T
where T: RaftLogId
{
    type CommittedLeaderId = T::CommittedLeaderId;

    fn committed_leader_id(&self) -> &Self::CommittedLeaderId {
        T::committed_leader_id(self)
    }

    fn index(&self) -> u64 {
        T::index(self)
    }
}
