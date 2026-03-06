use crate::Membership;
use crate::RaftPrimitives;
/// Defines operations on an entry payload.
pub trait RaftPayload<C>
where C: RaftPrimitives
{
    /// Return `Some(Membership)` if the entry payload contains a membership payload.
    fn get_membership(&self) -> Option<Membership<C>>;
}
