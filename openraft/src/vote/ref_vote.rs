use std::cmp::Ordering;
use std::fmt::Formatter;

use crate::RaftPrimitives;
use crate::Vote;
use crate::vote::RaftVote;

/// Similar to [`Vote`] but with a reference to the `LeaderId`, and provide ordering and display
/// implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RefVote<'a, P>
where P: RaftPrimitives
{
    pub(crate) leader_id: &'a P::LeaderId,
    pub(crate) committed: bool,
}

impl<'a, P> RefVote<'a, P>
where P: RaftPrimitives
{
    pub(crate) fn new(leader_id: &'a P::LeaderId, committed: bool) -> Self {
        Self { leader_id, committed }
    }

    pub(crate) fn is_committed(&self) -> bool {
        self.committed
    }

    /// Convert to an owned [`Vote`].
    #[allow(dead_code)]
    pub(crate) fn to_owned(&self) -> Vote<P> {
        Vote::from_leader_id(self.leader_id.clone(), self.committed)
    }
}

// Commit votes have a total order relation with all other votes
impl<'a, P> PartialOrd for RefVote<'a, P>
where P: RaftPrimitives
{
    #[inline]
    fn partial_cmp(&self, other: &RefVote<'a, P>) -> Option<Ordering> {
        match PartialOrd::partial_cmp(&self.leader_id, &other.leader_id) {
            Some(Ordering::Equal) => PartialOrd::partial_cmp(&self.committed, &other.committed),
            None => {
                // If two leader_id are not comparable, they won't both be granted(committed).
                // Therefore use `committed` to determine greatness to minimize election conflict.
                match (self.committed, other.committed) {
                    (false, false) => None,
                    (true, false) => Some(Ordering::Greater),
                    (false, true) => Some(Ordering::Less),
                    (true, true) => None,
                }
            }
            // Some(non-equal)
            cmp => cmp,
        }
    }
}

impl<P> std::fmt::Display for RefVote<'_, P>
where P: RaftPrimitives
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<{}:{}>",
            self.leader_id,
            if self.is_committed() { "Q" } else { "-" }
        )
    }
}
