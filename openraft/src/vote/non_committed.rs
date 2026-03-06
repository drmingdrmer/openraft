use std::fmt;

use crate::RaftPrimitives;
use crate::Vote;
use crate::type_config::alias::LeaderIdOf;
use crate::vote::RaftVote;
use crate::vote::leader_id::raft_leader_id::RaftLeaderIdExt;
use crate::vote::raft_vote::RaftVoteExt;

/// Represents a non-committed Vote that has **NOT** been granted by a quorum.
///
/// The inner `Vote`'s attribute `committed` is always set to `false`
#[derive(Debug, Clone)]
#[derive(PartialEq, Eq)]
#[derive(PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub(crate) struct UncommittedVote<P>
where P: RaftPrimitives
{
    leader_id: LeaderIdOf<P>,
}

impl<P> Default for UncommittedVote<P>
where
    P: RaftPrimitives,
    P::NodeId: Default,
{
    fn default() -> Self {
        Self {
            leader_id: LeaderIdOf::<P>::new_with_default_term(P::NodeId::default()),
        }
    }
}

impl<P> UncommittedVote<P>
where P: RaftPrimitives
{
    pub(crate) fn new(leader_id: LeaderIdOf<P>) -> Self {
        Self { leader_id }
    }

    pub(crate) fn into_vote<V: RaftVote<P>>(self) -> V {
        V::from_leader_id(self.leader_id, false)
    }

    pub(crate) fn into_internal_vote(self) -> Vote<P> {
        Vote::<P>::from_leader_id(self.leader_id, false)
    }
}

impl<P> RaftVote<P> for UncommittedVote<P>
where P: RaftPrimitives
{
    fn from_leader_id(leader_id: P::LeaderId, _committed: bool) -> Self {
        Self { leader_id }
    }

    fn leader_id(&self) -> &LeaderIdOf<P> {
        &self.leader_id
    }

    fn is_committed(&self) -> bool {
        false
    }
}

impl<P> fmt::Display for UncommittedVote<P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref_vote().fmt(f)
    }
}
