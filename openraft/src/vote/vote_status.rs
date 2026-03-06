use crate::RaftPrimitives;
use crate::vote::committed::CommittedVote;
use crate::vote::non_committed::UncommittedVote;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VoteStatus<C: RaftPrimitives> {
    Committed(CommittedVote<C>),
    Pending(UncommittedVote<C>),
}
