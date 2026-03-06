use crate::RaftPrimitives;
use crate::errors::ChangeMembershipError;
use crate::errors::EmptyMembership;
use crate::errors::LearnerNotFound;
use crate::errors::NodeNotFound;

/// Errors occur when building a [`Membership`].
///
/// [`Membership`]: crate::membership::Membership
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub enum MembershipError<P: RaftPrimitives> {
    /// The membership configuration is empty.
    #[error(transparent)]
    EmptyMembership(#[from] EmptyMembership),

    /// A required node was not found.
    #[error(transparent)]
    NodeNotFound(#[from] NodeNotFound<P>),
}

impl<P> From<MembershipError<P>> for ChangeMembershipError<P>
where P: RaftPrimitives
{
    fn from(me: MembershipError<P>) -> Self {
        match me {
            MembershipError::EmptyMembership(e) => ChangeMembershipError::EmptyMembership(e),
            MembershipError::NodeNotFound(e) => {
                ChangeMembershipError::LearnerNotFound(LearnerNotFound { node_id: e.node_id })
            }
        }
    }
}
