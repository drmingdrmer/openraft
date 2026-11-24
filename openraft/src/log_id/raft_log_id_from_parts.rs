use openraft_macros::since;

use crate::log_id::raft_log_id::RaftLogId;

/// A log ID that can be constructed from an owned committed leader ID and an index.
#[since(version = "0.10.2")]
pub trait RaftLogIdFromParts
where Self: RaftLogId
{
    /// Creates a log ID proposed by the committed leader `leader_id` at the given index.
    #[since(version = "0.10.2")]
    fn new(leader_id: Self::CommittedLeaderId, index: u64) -> Self;
}
