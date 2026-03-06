use crate::RaftComposites;
use crate::type_config::alias::VoteOf;

// TODO: remove it?
/// APIs to get vote.
#[allow(dead_code)]
pub(crate) trait VoteStateReader<C>
where C: RaftComposites
{
    /// Get a reference to the current vote.
    fn vote_ref(&self) -> &VoteOf<C>;
}
