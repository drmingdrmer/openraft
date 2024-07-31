use std::fmt::Debug;

pub trait RaftLeaderId<C>
where Self: Debug + Clone + Copy + PartialEq + Eq + PartialOrd
{
    type Committed: RaftCommittedLeaderId<C>;

    fn new(term: u64, node_id: C::NodeId) -> Self;

    fn get_term(&self) -> u64;

    fn voted_for(&self) -> Option<C::NodeId>;

    fn to_committed(&self) -> CommittedLeaderId<C::NodeId>;
}

pub trait RaftCommittedLeaderId<C> {}
