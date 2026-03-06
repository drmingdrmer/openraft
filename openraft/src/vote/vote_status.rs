use crate::RaftPrimitives;
use crate::vote::committed::CommittedVote;
use crate::vote::non_committed::UncommittedVote;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VoteStatus<P: RaftPrimitives> {
    Committed(CommittedVote<P>),
    Pending(UncommittedVote<P>),
}
