use std::fmt;

use crate::RaftPrimitives;
use crate::display_ext::DisplayOptionExt;
use crate::raft_state::io_state::IOId;
use crate::type_config::alias::LeaderIdOf;
use crate::type_config::alias::LogIdOf;
use crate::vote::RaftVote;
use crate::vote::committed::CommittedVote;

/// Monotonic increasing identifier for log append I/O operations.
///
/// `LogIOId = (CommittedVote, LogId)` ensures monotonicity even when the same [`LogId`] is
/// truncated and appended multiple times by different leaders.
///
/// For a comprehensive explanation, see: [`LogIOId` documentation](crate::docs::data::log_io_id).
///
/// [`LogId`]: crate::log_id::LogId
#[derive(Debug, Clone)]
#[derive(PartialEq, Eq)]
#[derive(PartialOrd, Ord)]
pub(crate) struct LogIOId<P>
where P: RaftPrimitives
{
    /// The id of the leader that performs the log io operation.
    pub(crate) committed_vote: CommittedVote<P>,

    /// The last log id that has been flushed to storage.
    pub(crate) log_id: Option<LogIdOf<P>>,
}

impl<P> fmt::Display for LogIOId<P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "by:{}, {}", self.committed_vote, self.log_id.display())
    }
}

impl<P> LogIOId<P>
where P: RaftPrimitives
{
    pub(crate) fn new(committed_vote: CommittedVote<P>, log_id: Option<LogIdOf<P>>) -> Self {
        Self { committed_vote, log_id }
    }

    #[allow(dead_code)]
    pub(crate) fn committed_vote(&self) -> &CommittedVote<P> {
        &self.committed_vote
    }

    pub(crate) fn to_committed_vote(&self) -> CommittedVote<P> {
        self.committed_vote.clone()
    }

    pub(crate) fn leader_id(&self) -> &LeaderIdOf<P> {
        self.committed_vote.leader_id()
    }

    /// Return the last log id included in this io operation.
    pub(crate) fn last_log_id(&self) -> Option<&LogIdOf<P>> {
        self.log_id.as_ref()
    }

    pub(crate) fn to_io_id(&self) -> IOId<P> {
        IOId::Log(self.clone())
    }
}
