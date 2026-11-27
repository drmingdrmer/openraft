use std::fmt::Display;
use std::fmt::Formatter;

use display_more::DisplayOptionExt;

use crate::RaftTypeConfig;
use crate::type_config::alias::CommittedVoteOf;
use crate::type_config::alias::LogIdOf;

/// Identifies the leader and effective membership when a heartbeat command is queued.
///
/// Before dispatching heartbeat events, the core checks that it is still the leader and that
/// both fields match its current state. Appending a new membership changes the effective log ID
/// before the membership is committed, invalidating commands queued with the previous ID.
///
/// A [`StreamId`] instead identifies a target's progress entry for its lifetime, which can span
/// effective membership changes.
///
/// [`StreamId`]: crate::progress::stream_id::StreamId
#[derive(Debug, Clone)]
#[derive(PartialEq, Eq)]
pub(crate) struct ReplicationSessionId<C>
where C: RaftTypeConfig
{
    /// The established leader's committed vote.
    pub(crate) leader_vote: CommittedVoteOf<C>,

    /// The log ID of the effective membership, which need not be committed.
    pub(crate) membership_log_id: Option<LogIdOf<C>>,
}

impl<C> Display for ReplicationSessionId<C>
where C: RaftTypeConfig
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "(leader_vote:{}, membership_log_id:{})",
            self.leader_vote,
            self.membership_log_id.display()
        )
    }
}

impl<C> ReplicationSessionId<C>
where C: RaftTypeConfig
{
    pub(crate) fn new(vote: CommittedVoteOf<C>, membership_log_id: Option<LogIdOf<C>>) -> Self {
        Self {
            leader_vote: vote,
            membership_log_id,
        }
    }
}
