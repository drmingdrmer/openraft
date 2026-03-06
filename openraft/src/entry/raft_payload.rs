use crate::Membership;
use crate::RaftPrimitives;
/// Defines operations on an entry payload.
pub trait RaftPayload<P>
where P: RaftPrimitives
{
    /// Return `Some(Membership)` if the entry payload contains a membership payload.
    fn get_membership(&self) -> Option<Membership<P>>;
}
