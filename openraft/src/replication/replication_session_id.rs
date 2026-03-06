use std::fmt::Display;
use std::fmt::Formatter;

use crate::RaftPrimitives;
use crate::display_ext::DisplayOptionExt;
use crate::type_config::alias::LogIdOf;
use crate::vote::committed::CommittedVote;

/// Uniquely identifies a replication session.
///
/// A replication session represents a set of replication streams from a leader to its followers.
/// For example, in a cluster of 3 nodes where node-1 is the leader, it maintains replication
/// streams to nodes {2,3}.
///
/// A replication session is uniquely identified by the leader's vote and the membership
/// configuration. When either changes, a new replication session is created.
///
/// See: [ReplicationSession](crate::docs::data::replication_session)
#[derive(Debug, Clone)]
#[derive(PartialEq, Eq)]
pub(crate) struct ReplicationSessionId<P>
where P: RaftPrimitives
{
    /// The Leader or Candidate this replication belongs to.
    pub(crate) leader_vote: CommittedVote<P>,

    /// The log id of the membership log this replication works for.
    pub(crate) membership_log_id: Option<LogIdOf<P>>,
}

impl<P> Display for ReplicationSessionId<P>
where P: RaftPrimitives
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

impl<P> ReplicationSessionId<P>
where P: RaftPrimitives
{
    pub(crate) fn new(vote: CommittedVote<P>, membership_log_id: Option<LogIdOf<P>>) -> Self {
        Self {
            leader_vote: vote,
            membership_log_id,
        }
    }
}
