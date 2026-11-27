use std::fmt;

use display_more::DisplayOptionExt;

use crate::RaftTypeConfig;
use crate::raft_state::io_state::IOId;
use crate::type_config::alias::CommittedVoteOf;
use crate::type_config::alias::LeaderIdOf;
use crate::type_config::alias::LogIdOf;
use crate::vote::RaftVote;

/// A committed-vote boundary with optional log coverage: `(CommittedVote, Option<LogId>)`.
///
/// Comparison uses the full [`CommittedVote`] order, not just the term. Only when the votes
/// compare equal does it compare the optional [`LogId`], with `None` before `Some`. This lets an
/// append under a greater vote advance a cursor even if the same log ID was truncated and appended
/// again. Callers keep their cursors nondecreasing; individual values can repeat or arrive stale.
///
/// The [`IOProgress`] cursor that holds the boundary determines whether it is accepted, submitted,
/// or flushed. The value alone does not establish local durability: it can describe a snapshot
/// awaiting installation, a committed vote without a log endpoint, or a reported cluster commit
/// beyond the locally accepted log.
///
/// For a comprehensive explanation, see: [`LogIOId` documentation](crate::docs::data::log_io_id).
///
/// [`CommittedVote`]: crate::vote::committed::CommittedVote
/// [`IOProgress`]: crate::raft_state::io_state::io_progress::IOProgress
/// [`LogId`]: crate::log_id::LogId
#[derive(Debug, Clone)]
#[derive(PartialEq, Eq)]
#[derive(PartialOrd, Ord)]
pub(crate) struct LogIOId<C>
where C: RaftTypeConfig
{
    /// The vote of the leader authorizing the I/O or reporting the cluster commit, which need
    /// not be the leader that originally proposed the entry.
    pub(crate) committed_vote: CommittedVoteOf<C>,

    /// The log endpoint carried by this boundary. `None` means the boundary carries no log
    /// coverage, not that the local store is empty.
    pub(crate) log_id: Option<LogIdOf<C>>,
}

impl<C> fmt::Display for LogIOId<C>
where C: RaftTypeConfig
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "by:{}, {}", self.committed_vote, self.log_id.display())
    }
}

impl<C> LogIOId<C>
where C: RaftTypeConfig
{
    pub(crate) fn new(committed_vote: CommittedVoteOf<C>, log_id: Option<LogIdOf<C>>) -> Self {
        Self { committed_vote, log_id }
    }

    #[allow(dead_code)]
    pub(crate) fn committed_vote(&self) -> &CommittedVoteOf<C> {
        &self.committed_vote
    }

    pub(crate) fn to_committed_vote(&self) -> CommittedVoteOf<C> {
        self.committed_vote.clone()
    }

    pub(crate) fn leader_id(&self) -> &LeaderIdOf<C> {
        self.committed_vote.leader_id()
    }

    /// Return the log endpoint carried by this boundary, if any.
    pub(crate) fn last_log_id(&self) -> Option<&LogIdOf<C>> {
        self.log_id.as_ref()
    }

    pub(crate) fn to_io_id(&self) -> IOId<C> {
        IOId::Log(self.clone())
    }
}
